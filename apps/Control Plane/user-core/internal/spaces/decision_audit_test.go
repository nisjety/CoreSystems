package spaces

import (
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// WS05-S5: the audit flags and the content-free event builders.

func envFrom(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

func TestLoadDecisionAuditConfig(t *testing.T) {
	cases := []struct {
		name string
		env  map[string]string
		want DecisionAuditConfig
	}{
		{"unset is off", nil, DecisionAuditConfig{}},
		{"not the strict literal", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": "1"}, DecisionAuditConfig{}},
		{"True is not the strict literal", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": "True"}, DecisionAuditConfig{}},
		{"kinds without the switch stay off", map[string]string{"SPACE_AUDIT_DECISION_KINDS": "denied,issued"}, DecisionAuditConfig{}},
		{"kinds default to denied", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": "true"}, DecisionAuditConfig{Enabled: true, Denied: true}},
		{"blank kinds default to denied", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": " true ", "SPACE_AUDIT_DECISION_KINDS": "  "}, DecisionAuditConfig{Enabled: true, Denied: true}},
		{"denied only", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": "true", "SPACE_AUDIT_DECISION_KINDS": "denied"}, DecisionAuditConfig{Enabled: true, Denied: true}},
		{"issued only", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": "true", "SPACE_AUDIT_DECISION_KINDS": "issued"}, DecisionAuditConfig{Enabled: true, Issued: true}},
		{"both, case and space tolerant", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": "true", "SPACE_AUDIT_DECISION_KINDS": " Denied , ISSUED "}, DecisionAuditConfig{Enabled: true, Denied: true, Issued: true}},
		{"unknown token enables nothing", map[string]string{"SPACE_AUDIT_DECISION_EVENTS": "true", "SPACE_AUDIT_DECISION_KINDS": "everything"}, DecisionAuditConfig{Enabled: true}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := LoadDecisionAuditConfig(envFrom(tc.env)); got != tc.want {
				t.Fatalf("config = %+v, want %+v", got, tc.want)
			}
		})
	}
}

func TestDecisionAuditRouteForMatchesTheIssuerSuffix(t *testing.T) {
	route, row, ok := DecisionAuditRouteFor("/api/v1/internal/spaces/run-control-decision")
	if !ok || route != "/run-control-decision" || row.Name != "run_control" || !row.EffectBound {
		t.Fatalf("run-control = %q %+v %v", route, row, ok)
	}
	_, row, ok = DecisionAuditRouteFor("/api/v1/internal/spaces/thread-read-decision")
	if !ok || row.EffectBound {
		t.Fatalf("a thread read must be audited on denial only: %+v %v", row, ok)
	}
	if _, _, ok := DecisionAuditRouteFor("/api/v1/internal/spaces/membership"); ok {
		t.Fatal("a non-issuer path must not resolve")
	}
	if _, _, ok := DecisionAuditRouteFor("/api/v1/users/me"); ok {
		t.Fatal("a path outside /internal/spaces must not resolve")
	}
}

func TestEffectBoundRoutesAreExactlyTheEffectKinds(t *testing.T) {
	for route, row := range decisionAuditRoutes {
		switch row.Name {
		case "thread_create", "thread_append", "thread_read", "retrieval", "watch_observe", "run_wake":
			if row.EffectBound {
				t.Errorf("%s (%s) is a read/append/wake and must not be audited as issued", route, row.Name)
			}
		default:
			if !row.EffectBound {
				t.Errorf("%s (%s) is an effect kind and must be effect-bound", route, row.Name)
			}
		}
	}
}

func deniedFixture(at time.Time) DeniedDecisionAudit {
	return DeniedDecisionAudit{
		Route: "/thread-decision", Kind: "thread_create", OrgID: "org-1", SubjectID: "user-1",
		SpaceRef: "space-1", ActionID: "model.run.create", Status: 403,
		Reason: "space_role_insufficient", ResolvableBy: "space_manager", OccurredAt: at,
	}
}

func decodeAudit(t *testing.T, payload []byte) map[string]any {
	t.Helper()
	var body map[string]any
	if err := json.Unmarshal(payload, &body); err != nil {
		t.Fatalf("payload is not JSON: %v", err)
	}
	return body
}

func TestBuildDeniedDecisionAuditIDCollapsesRetriesPerMinute(t *testing.T) {
	at := time.Date(2026, 11, 2, 10, 15, 5, 0, time.UTC)
	id1, subject, payload, err := BuildDeniedDecisionAudit(deniedFixture(at))
	if err != nil {
		t.Fatalf("BuildDeniedDecisionAudit: %v", err)
	}
	if !strings.HasPrefix(id1, "space-denial:") || len(id1) != len("space-denial:")+64 {
		t.Fatalf("event id = %q, want space-denial:<sha256 hex>", id1)
	}
	if subject != AuditSubjectSpaceDecisionDenied || !strings.HasPrefix(subject, "verevon.audit.v2.control.user-core.") {
		t.Fatalf("subject = %q", subject)
	}

	id2, _, _, _ := BuildDeniedDecisionAudit(deniedFixture(at.Add(40 * time.Second)))
	if id1 != id2 {
		t.Fatal("a retry inside the same minute must collapse to one id")
	}
	id3, _, _, _ := BuildDeniedDecisionAudit(deniedFixture(at.Add(2 * time.Minute)))
	if id1 == id3 {
		t.Fatal("a later minute is a new row")
	}
	other := deniedFixture(at)
	other.SpaceRef = "space-2"
	if id4, _, _, _ := BuildDeniedDecisionAudit(other); id4 == id1 {
		t.Fatal("a different Space is a different row")
	}
	other = deniedFixture(at)
	other.Route = "/retrieval-decision"
	if id5, _, _, _ := BuildDeniedDecisionAudit(other); id5 == id1 {
		t.Fatal("a different route is a different row")
	}

	body := decodeAudit(t, payload)
	if body["outcome"] != "denied" || body["event"] != "space_decision_denied" || body["event_id"] != id1 {
		t.Fatalf("body = %v", body)
	}
	if body["org_id"] != "org-1" || body["user_id"] != "user-1" || body["subject"] != "space:space-1" || body["resource_id"] != "space-1" {
		t.Fatalf("identifiers = %v", body)
	}
	if body["plane"] != "control" || body["producer"] != "user-core" {
		t.Fatalf("producer = %v", body)
	}
	details := body["details"].(map[string]any)
	if details["decision_kind"] != "thread_create" || details["reason"] != "space_role_insufficient" ||
		details["resolvable_by"] != "space_manager" || details["action_id"] != "model.run.create" || details["status"] != float64(403) {
		t.Fatalf("details = %v", details)
	}
}

func TestBuildDeniedDecisionAuditNeedsAnOrgAndOmitsEmptyFields(t *testing.T) {
	in := deniedFixture(time.Now())
	in.OrgID = "  "
	if _, _, _, err := BuildDeniedDecisionAudit(in); err == nil {
		t.Fatal("a denial without an org must not be built")
	}
	in = deniedFixture(time.Now())
	in.Reason, in.ResolvableBy, in.ActionID, in.SubjectID, in.SpaceRef = "", "", "", "", ""
	_, _, payload, err := BuildDeniedDecisionAudit(in)
	if err != nil {
		t.Fatalf("an untyped refusal is still audited: %v", err)
	}
	body := decodeAudit(t, payload)
	details := body["details"].(map[string]any)
	for _, key := range []string{"reason", "resolvable_by", "action_id"} {
		if _, present := details[key]; present {
			t.Errorf("empty %s must be omitted", key)
		}
	}
	for _, key := range []string{"user_id", "subject", "resource_id"} {
		if _, present := body[key]; present {
			t.Errorf("empty %s must be omitted", key)
		}
	}
}

func issuedFixture(at time.Time) IssuedDecisionAudit {
	return IssuedDecisionAudit{
		Route: "/run-control-decision", Kind: "run_control", DecisionRef: "dec-abc",
		OrgID: "org-1", SubjectID: "user-1", SpaceRef: "space-1", ActionID: "model.run.control",
		ServiceAudience: "model-plane", AuthorityRevision: 7, ExpiresAt: time.Date(2026, 11, 2, 10, 20, 0, 0, time.UTC),
		ZeroDataRetention: true, OccurredAt: time.Date(2026, 11, 2, 10, 15, 0, 0, time.UTC),
	}
}

func TestBuildIssuedDecisionAuditIsKeyedByTheDecisionRef(t *testing.T) {
	id, subject, payload, err := BuildIssuedDecisionAudit(issuedFixture(time.Now()))
	if err != nil {
		t.Fatalf("BuildIssuedDecisionAudit: %v", err)
	}
	if id != "space-decision:dec-abc" || subject != AuditSubjectSpaceDecisionIssued {
		t.Fatalf("id=%q subject=%q", id, subject)
	}
	body := decodeAudit(t, payload)
	if body["outcome"] != "ok" || body["event"] != "space_decision_issued" {
		t.Fatalf("body = %v", body)
	}
	details := body["details"].(map[string]any)
	if details["service_audience"] != "model-plane" || details["authority_revision"] != float64(7) ||
		details["zero_data_retention"] != true || details["expires_at"] != "2026-11-02T10:20:00Z" {
		t.Fatalf("details = %v", details)
	}

	// A replay (later clock) cannot produce a second row.
	again := issuedFixture(time.Now().Add(time.Hour))
	if id2, _, _, _ := BuildIssuedDecisionAudit(again); id2 != id {
		t.Fatal("the id must depend on the decision reference only")
	}

	in := issuedFixture(time.Now())
	in.DecisionRef = ""
	if _, _, _, err := BuildIssuedDecisionAudit(in); err == nil {
		t.Fatal("no decision reference must not build")
	}
	in = issuedFixture(time.Now())
	in.OrgID = ""
	if _, _, _, err := BuildIssuedDecisionAudit(in); err == nil {
		t.Fatal("no org must not build")
	}
	in = issuedFixture(time.Now())
	in.DecisionRef = strings.Repeat("x", 200)
	if _, _, _, err := BuildIssuedDecisionAudit(in); err == nil {
		t.Fatal("an over-long decision reference must not build")
	}
}

func TestDecisionAuditDetailsCarryNoTokenDigestNonceOrIdempotencyKey(t *testing.T) {
	_, _, issued, _ := BuildIssuedDecisionAudit(issuedFixture(time.Now()))
	_, _, denied, _ := BuildDeniedDecisionAudit(deniedFixture(time.Now()))
	for name, payload := range map[string][]byte{"issued": issued, "denied": denied} {
		text := strings.ToLower(string(payload))
		for _, forbidden := range []string{"token", "digest", "nonce", "idempotency", "recipient", "payload"} {
			if strings.Contains(text, forbidden) {
				t.Errorf("%s audit payload mentions %q: %s", name, forbidden, payload)
			}
		}
	}
}

func TestAuditStringsAreClamped(t *testing.T) {
	in := deniedFixture(time.Now())
	in.Reason = strings.Repeat("r", 500)
	in.SpaceRef = strings.Repeat("s", 500)
	_, _, payload, err := BuildDeniedDecisionAudit(in)
	if err != nil {
		t.Fatal(err)
	}
	body := decodeAudit(t, payload)
	if got := len(body["details"].(map[string]any)["reason"].(string)); got != maxAuditDetailString {
		t.Fatalf("reason length = %d, want %d", got, maxAuditDetailString)
	}
	if got := len(body["resource_id"].(string)); got != maxAuditDetailString {
		t.Fatalf("resource_id length = %d, want %d", got, maxAuditDetailString)
	}
}
