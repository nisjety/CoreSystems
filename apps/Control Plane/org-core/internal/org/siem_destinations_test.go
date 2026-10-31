package org

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

func TestValidateSIEMURL(t *testing.T) {
	valid := []string{
		"https://siem.example.com/ingest",
		"https://siem.example.com:8443/services/collector/event?channel=abc",
		"https://203.0.113.10/hook",
		"https://[2001:4860:4860::8888]/hook",
		"https://splunk.example.co.uk:8088/services/collector",
	}
	for _, raw := range valid {
		if problems := validateSIEMURL(raw); len(problems) != 0 {
			t.Errorf("%q rejected: %v", raw, problems)
		}
	}

	invalid := map[string]string{
		"":                                     "required",
		"http://siem.example.com/x":            "https",
		"ftp://siem.example.com/x":             "https",
		"https://user:pw@siem.example.com/x":   "credentials",
		"https://user@siem.example.com/x":      "credentials",
		"https://siem.example.com/x#frag":      "fragment",
		"https://siem.example.com:0/x":         "port",
		"https://siem.example.com:70000/x":     "port",
		"https:///path":                        "host",
		"https://localhost/x":                  "internal",
		"https://app.localhost/x":              "internal",
		"https://db.internal/x":                "internal",
		"https://printer.local/x":              "internal",
		"https://127.0.0.1/x":                  "private",
		"https://10.1.2.3/x":                   "private",
		"https://172.16.0.1/x":                 "private",
		"https://192.168.1.1/x":                "private",
		"https://169.254.169.254/latest":       "private",
		"https://100.64.0.1/x":                 "private",
		"https://0.0.0.0/x":                    "private",
		"https://[::1]/x":                      "private",
		"https://[fe80::1]/x":                  "private",
		"https://[fd00::1]/x":                  "private",
		"https://[::ffff:10.0.0.1]/x":          "private",
		"https://[::ffff:169.254.169.254]/x":   "private",
		"https://2130706433/x":                 "hostname",
		"https://0x7f.0.0.1/x":                 "hostname",
		"https://siem.example.com/ x":          "whitespace",
		" https://siem.example.com/x":          "whitespace",
		"https://siem.example.com\\@evil.com":  "whitespace",
		"https://" + strings.Repeat("a", 2050): "2048",
	}
	for raw, want := range invalid {
		problems := validateSIEMURL(raw)
		if len(problems) == 0 {
			t.Errorf("%q was accepted; want a %q problem", raw, want)
			continue
		}
		if !strings.Contains(strings.Join(problems, ";"), want) {
			t.Errorf("%q problems %v do not mention %q", raw, problems, want)
		}
		// A validation message never echoes what was sent.
		if raw != "" && strings.Contains(strings.Join(problems, ";"), strings.TrimSpace(raw)) {
			t.Errorf("%q: the problem list echoes the URL: %v", raw[:min(len(raw), 40)], problems)
		}
	}
}

func TestValidateSIEMSecret(t *testing.T) {
	for _, ok := range []string{"0123456789abcdef", strings.Repeat("a", 512), "a-hec-token-with-dashes-1234"} {
		if problems := validateSIEMSecret(ok); len(problems) != 0 {
			t.Errorf("secret of %d bytes rejected: %v", len(ok), problems)
		}
	}
	for name, secret := range map[string]string{
		"short":      "0123456789abcde",
		"empty":      "",
		"long":       strings.Repeat("a", 513),
		"leading":    " 0123456789abcdef",
		"trailing":   "0123456789abcdef\n",
		"control":    "0123456789abc\x00def",
		"delete":     "0123456789abc\x7fdef",
		"all spaces": strings.Repeat(" ", 20),
	} {
		problems := validateSIEMSecret(secret)
		if len(problems) == 0 {
			t.Errorf("%s secret accepted", name)
		}
		if secret != "" && strings.Contains(strings.Join(problems, ";"), strings.TrimSpace(secret)) && strings.TrimSpace(secret) != "" {
			t.Errorf("%s: the problem list echoes the secret", name)
		}
	}
}

func TestValidateSIEMFilter(t *testing.T) {
	if problems := validateSIEMFilter(SIEMEventFilter{Events: []string{"tool.call", "org_siem_destination_changed"}, Planes: []string{"control", "model"}}); len(problems) != 0 {
		t.Fatalf("valid filter rejected: %v", problems)
	}
	tooMany := make([]string, siemFilterMaxItems+1)
	for i := range tooMany {
		tooMany[i] = "e"
	}
	for name, filter := range map[string]SIEMEventFilter{
		"too many events": {Events: tooMany},
		"too many planes": {Planes: tooMany},
		"empty entry":     {Events: []string{""}},
		"space":           {Events: []string{"has space"}},
		"too long":        {Planes: []string{strings.Repeat("a", 129)}},
	} {
		if problems := validateSIEMFilter(filter); len(problems) == 0 {
			t.Errorf("%s accepted", name)
		}
	}
	got := normalizeSIEMFilter(SIEMEventFilter{Events: []string{"a", "b", "a"}})
	if len(got.Events) != 2 || got.Planes != nil {
		t.Fatalf("normalize = %+v; want deduped events and no planes", got)
	}
	encoded, _ := json.Marshal(normalizeSIEMFilter(SIEMEventFilter{}))
	if string(encoded) != "{}" {
		t.Fatalf("an empty filter stores as %s, want {}", encoded)
	}
}

func TestSIEMExportIsAnEnterprisePlanFeature(t *testing.T) {
	for plan, want := range map[string]bool{
		"enterprise": true, " Enterprise ": true, "pro": false, "standard": false, "free": false, "trial": false, "": false,
	} {
		if got := PlanAllowsSIEMExport(plan); got != want {
			t.Errorf("PlanAllowsSIEMExport(%q) = %v, want %v", plan, got, want)
		}
	}
}

func TestSIEMAuditPayloadIsContentFree(t *testing.T) {
	destination := SIEMDestination{
		ID: "d-1", OrgID: "org-1", Kind: SIEMKindSplunkHEC, Enabled: true, Revision: 3,
		// The query string carries a credential-shaped value on purpose.
		URL: "https://Splunk.Example.com:8088/services/collector?token=SECRET-IN-QUERY&channel=c1",
	}
	eventID, payload := siemAuditPayload(siemAuditChange{
		Action: "updated", Destination: destination, ActorID: "u-1", ActorRole: "admin",
		SecretRotated: true, EventRevision: 3,
	}, time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC))

	if eventID != "siem:org-core:d-1:3" || payload["event_id"] != eventID {
		t.Fatalf("event id = %q payload id = %v", eventID, payload["event_id"])
	}
	for key, want := range map[string]any{
		"org_id": "org-1", "user_id": "u-1", "actor_role": "admin", "plane": "control",
		"producer": "org-core", "event": SIEMAuditEvent, "outcome": "ok", "resource_id": "d-1",
	} {
		if payload[key] != want {
			t.Errorf("payload[%s] = %v, want %v", key, payload[key], want)
		}
	}
	details, _ := payload["details"].(map[string]any)
	if details["host"] != "splunk.example.com" || details["action"] != "updated" || details["secret_rotated"] != true {
		t.Fatalf("details = %v", details)
	}
	encoded, _ := json.Marshal(payload)
	for _, leaked := range []string{"SECRET-IN-QUERY", "channel", "8088", "services/collector", "token"} {
		if strings.Contains(string(encoded), leaked) {
			t.Fatalf("the audit payload carries %q: %s", leaked, encoded)
		}
	}
	if SIEMAuditSubject != "verevon.audit.v2.control.org-core.org_siem_destination_changed" {
		t.Fatalf("audit subject = %q", SIEMAuditSubject)
	}
}

func TestNewSIEMDestinationIDIsAVersion4UUID(t *testing.T) {
	seen := map[string]bool{}
	for i := 0; i < 50; i++ {
		id, err := newSIEMDestinationID()
		if err != nil {
			t.Fatalf("id: %v", err)
		}
		if len(id) != 36 || id[8] != '-' || id[13] != '-' || id[18] != '-' || id[23] != '-' || id[14] != '4' {
			t.Fatalf("id %q is not a v4 uuid", id)
		}
		if seen[id] {
			t.Fatalf("duplicate id %q", id)
		}
		seen[id] = true
	}
}

// A service without a seal key is the feature-off state: every SIEM method
// refuses before touching a database, and the entitlement is not reported.
func TestSIEMMethodsRefuseWithoutASealer(t *testing.T) {
	service := NewService(nil, nil)
	if service.SIEMDestinationsEnabled() {
		t.Fatalf("a new service reports SIEM enabled")
	}
	ctx := context.Background()
	if _, err := service.ListSIEMDestinations(ctx, "o"); !errors.Is(err, ErrSIEMSealUnavailable) {
		t.Fatalf("list: %v", err)
	}
	if _, err := service.CreateSIEMDestination(ctx, "o", "u", "owner", CreateSIEMDestinationRequest{}); !errors.Is(err, ErrSIEMSealUnavailable) {
		t.Fatalf("create: %v", err)
	}
	if _, err := service.UpdateSIEMDestination(ctx, "o", "d", "u", "owner", UpdateSIEMDestinationRequest{}); !errors.Is(err, ErrSIEMSealUnavailable) {
		t.Fatalf("update: %v", err)
	}
	if err := service.DeleteSIEMDestination(ctx, "o", "d", "u", "owner"); !errors.Is(err, ErrSIEMSealUnavailable) {
		t.Fatalf("delete: %v", err)
	}
	if _, err := service.ListSIEMDestinationsForExport(ctx, nil); !errors.Is(err, ErrSIEMSealUnavailable) {
		t.Fatalf("export: %v", err)
	}
}

// Validation runs before the repository is touched, so a malformed request
// never reaches the database (the service here has no repository at all).
func TestSIEMRequestsAreValidatedBeforeAnyStorage(t *testing.T) {
	service := NewService(nil, nil)
	service.EnableSIEMDestinations(mustSealer(t, "v1", 0x33))
	ctx := context.Background()

	_, err := service.CreateSIEMDestination(ctx, "o", "u", "owner", CreateSIEMDestinationRequest{
		Kind: "syslog", URL: "http://127.0.0.1/x", Secret: "short",
	})
	var invalid *SIEMValidationError
	if !errors.As(err, &invalid) || len(invalid.Errors) < 4 {
		t.Fatalf("create err = %v; want a validation error listing kind, url (twice) and secret", err)
	}
	if strings.Contains(err.Error(), "127.0.0.1") || strings.Contains(err.Error(), "short") {
		t.Fatalf("the error echoes the request: %v", err)
	}

	for name, req := range map[string]UpdateSIEMDestinationRequest{
		"no revision": {Secret: ptr("0123456789abcdef")},
		"no change":   {ExpectedRevision: 1},
		"bad url":     {ExpectedRevision: 1, URL: ptr("http://x.example.com")},
		"bad secret":  {ExpectedRevision: 1, Secret: ptr("tiny")},
	} {
		if _, err := service.UpdateSIEMDestination(ctx, "o", "d", "u", "owner", req); !errors.As(err, &invalid) {
			t.Fatalf("%s: err = %v; want a validation error", name, err)
		}
	}
}

func ptr[T any](v T) *T { return &v }
