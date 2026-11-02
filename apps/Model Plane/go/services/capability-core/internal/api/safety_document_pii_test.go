package api

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgconn"
)

// E3.6-8 (capability-core half): the document_pii catalogue key of safety_policies. Written
// with the code; the whole key is dark behind DOCUMENT_PII_POLICY.

func TestDocumentPIIPolicyFromLookup_OnlyTheLiteralTrueTurnsItOn(t *testing.T) {
	for raw, want := range map[string]bool{
		"true": true, "": false, "1": false, "TRUE": false, "True": false, "on": false, "yes": false, "false": false, " true": false,
	} {
		lookup := func(name string) string {
			if name != "DOCUMENT_PII_POLICY" {
				t.Fatalf("read %q, want DOCUMENT_PII_POLICY", name)
			}
			return raw
		}
		if got := DocumentPIIPolicyFromLookup(lookup); got != want {
			t.Errorf("%q: %v, want %v", raw, got, want)
		}
	}
}

func TestParseDocumentPIIConfig_IsClosed(t *testing.T) {
	good := map[string]DocumentPIIConfig{
		`{"mode":"off"}`:                            {Mode: "off"},
		`{"mode":"redact"}`:                         {Mode: "redact"},
		`{"mode":"fail_closed"}`:                    {Mode: "fail_closed"},
		`{"mode":"fail_closed","model_tier":true}`:  {Mode: "fail_closed", ModelTier: true},
		`{"mode":"redact","model_tier":false}`:      {Mode: "redact"},
		` { "mode" : "redact" , "model_tier":true}`: {Mode: "redact", ModelTier: true},
	}
	for raw, want := range good {
		got, err := ParseDocumentPIIConfig([]byte(raw))
		if err != nil || got != want {
			t.Errorf("%s: %+v, %v; want %+v", raw, got, err, want)
		}
	}
	for _, raw := range []string{
		``, `null`, `{}`, `[]`, `"off"`, `{"mode":""}`, `{"mode":"FAIL_CLOSED"}`, `{"mode":"strict"}`,
		`{"mode":"off","model_tier":true}`,                // a model pass needs a stage to follow
		`{"mode":"redact","unknown":1}`,                   // a typo must not become a weaker policy
		`{"mode":"redact","model_tier":"yes"}`,            // wrong type
		`{"mode":"redact"} {"mode":"off"}`,                // trailing data
		`{"mode":"redact","mode":"off"}x`,                 // trailing junk
		`{"mode":"redact","entities":["email","phone"]}`,  // entity lists are not part of the key
		strings.Repeat(" ", maxDocumentPIIConfigBytes+1),  // oversized
	} {
		if cfg, err := ParseDocumentPIIConfig([]byte(raw)); err == nil {
			t.Errorf("%q was accepted as %+v", raw, cfg)
		}
	}
}

func TestEffectiveDocumentPII_OrgBeatsGlobalAndABrokenRowFailsClosed(t *testing.T) {
	row := func(org string, enabled bool, cfg any) safetyPolicyRow {
		return safetyPolicyRow{ID: "sp_" + org, OrgID: org, Name: "document_pii_default", Kind: "document_pii", ConfigJSON: cfg, Enabled: enabled}
	}
	cases := []struct {
		name string
		rows []safetyPolicyRow
		want documentPIIEffective
	}{
		{"nothing at all", nil, documentPIIEffective{Mode: "off", Source: "default"}},
		{"global only", []safetyPolicyRow{row("global", true, map[string]any{"mode": "redact"})},
			documentPIIEffective{Mode: "redact", Source: "global"}},
		{"org beats global", []safetyPolicyRow{
			row("global", true, map[string]any{"mode": "off"}),
			row("org-a", true, map[string]any{"mode": "fail_closed", "model_tier": true}),
		}, documentPIIEffective{Mode: "fail_closed", ModelTier: true, Source: "org"}},
		{"a disabled org row falls back to global", []safetyPolicyRow{
			row("global", true, map[string]any{"mode": "redact"}),
			row("org-a", false, map[string]any{"mode": "fail_closed"}),
		}, documentPIIEffective{Mode: "redact", Source: "global"}},
		{"another org's row is not read", []safetyPolicyRow{
			row("org-b", true, map[string]any{"mode": "fail_closed"}),
		}, documentPIIEffective{Mode: "off", Source: "default"}},
		{"a disabled global row is no policy", []safetyPolicyRow{
			row("global", false, map[string]any{"mode": "fail_closed"}),
		}, documentPIIEffective{Mode: "off", Source: "default"}},
		{"a broken org row reads as fail_closed", []safetyPolicyRow{
			row("global", true, map[string]any{"mode": "off"}),
			row("org-a", true, map[string]any{"mode": "strict"}),
		}, documentPIIEffective{Mode: "fail_closed", Source: "org_invalid"}},
		{"a broken global row reads as fail_closed", []safetyPolicyRow{
			row("global", true, "garbage"),
		}, documentPIIEffective{Mode: "fail_closed", Source: "global_invalid"}},
		{"other kinds are ignored", []safetyPolicyRow{
			{ID: "x", OrgID: "org-a", Kind: "pii_filter", Enabled: true, ConfigJSON: map[string]any{"mode": "fail_closed"}},
		}, documentPIIEffective{Mode: "off", Source: "default"}},
	}
	for _, c := range cases {
		if got := effectiveDocumentPII(c.rows, "org-a"); got != c.want {
			t.Errorf("%s: %+v, want %+v", c.name, got, c.want)
		}
	}
}

func documentPIIPost(t *testing.T, database *recordingDatabase, enabled bool, body string) *httptest.ResponseRecorder {
	t.Helper()
	safety := NewSafetyHandler(nil).WithDocumentPIIPolicy(enabled)
	safety.pool = database
	mux := http.NewServeMux()
	safety.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)
	request := httptest.NewRequest(http.MethodPost, "/api/v1/safety", strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	request.Header.Set("Content-Type", "application/json")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

func TestSafetyDocumentPIIPolicyIsDarkByDefault(t *testing.T) {
	database := &recordingDatabase{}
	response := documentPIIPost(t, database, false, `{"kind":"document_pii","config_json":{"mode":"fail_closed"},"enabled":true}`)
	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, body=%s, want 404 while DOCUMENT_PII_POLICY is off", response.Code, response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatalf("the database saw %d statements while the key is off", database.databaseCalls())
	}
	// The switch is a case-insensitive kind match: no spelling reaches the table.
	response = documentPIIPost(t, database, false, `{"kind":" Document_PII ","config_json":{"mode":"fail_closed"}}`)
	if response.Code != http.StatusNotFound || database.databaseCalls() != 0 {
		t.Fatalf("status = %d with %d statements, want 404 and none", response.Code, database.databaseCalls())
	}
	// Other kinds are unchanged by the flag.
	response = documentPIIPost(t, database, false, `{"name":"pii","kind":"pii_filter","config_json":{},"enabled":true}`)
	if response.Code != http.StatusCreated || len(database.execs) != 1 {
		t.Fatalf("pii_filter: status = %d, writes = %d, want 201 and one write", response.Code, len(database.execs))
	}
	// And the effective route is a 404 while off.
	safety := NewSafetyHandler(nil)
	safety.pool = database
	mux := http.NewServeMux()
	safety.Register(mux)
	handler, readToken, _ := mcpAuthenticatedHandler(t, mux)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/safety/document-pii/effective", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	effective := httptest.NewRecorder()
	handler.ServeHTTP(effective, request)
	if effective.Code != http.StatusNotFound {
		t.Fatalf("effective: status = %d, want 404 while off", effective.Code)
	}
}

func TestSafetyDocumentPIIPolicyIsValidatedAndNormalized(t *testing.T) {
	database := &recordingDatabase{}
	response := documentPIIPost(t, database, true,
		`{"kind":"Document_PII","name":"document_pii_default","config_json":{"mode":"fail_closed","model_tier":true},"applies_to":["document"],"enabled":true}`)
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s, want 201", response.Code, response.Body.String())
	}
	if len(database.execs) != 1 {
		t.Fatalf("writes = %d, want one", len(database.execs))
	}
	args := database.execs[0].args
	// id, org_id, name, description, kind, config_json, applies_to, ...
	if args[1] != "org-a" || args[2] != "document_pii_default" || args[4] != "document_pii" {
		t.Fatalf("stored org/name/kind = %v / %v / %v", args[1], args[2], args[4])
	}
	if got, ok := args[5].([]byte); !ok || string(got) != `{"mode":"fail_closed","model_tier":true}` {
		t.Fatalf("stored config = %#v", args[5])
	}
	if got, ok := args[6].([]string); !ok || len(got) != 1 || got[0] != "document" {
		t.Fatalf("stored applies_to = %#v", args[6])
	}

	// Name and applies_to default in; a name or scope of another kind is refused.
	database = &recordingDatabase{}
	if r := documentPIIPost(t, database, true, `{"kind":"document_pii","config_json":{"mode":"redact"},"enabled":true}`); r.Code != http.StatusCreated {
		t.Fatalf("defaults: status = %d, body=%s", r.Code, r.Body.String())
	}
	if args := database.execs[0].args; args[2] != "document_pii_default" {
		t.Fatalf("default name = %v", args[2])
	}
	for _, body := range []string{
		`{"kind":"document_pii","config_json":{"mode":"strict"}}`,
		`{"kind":"document_pii","config_json":{}}`,
		`{"kind":"document_pii"}`,
		`{"kind":"document_pii","config_json":{"mode":"off","model_tier":true}}`,
		`{"kind":"document_pii","config_json":{"mode":"redact","extra":1}}`,
		`{"kind":"document_pii","name":"mine","config_json":{"mode":"redact"}}`,
		`{"kind":"document_pii","applies_to":["input"],"config_json":{"mode":"redact"}}`,
	} {
		refused := &recordingDatabase{}
		if r := documentPIIPost(t, refused, true, body); r.Code != http.StatusBadRequest || refused.databaseCalls() != 0 {
			t.Errorf("%s: status = %d with %d statements, want 400 and none", body, r.Code, refused.databaseCalls())
		}
	}
}

func TestSafetyDocumentPIIPolicyIsOneRowPerOrg(t *testing.T) {
	database := &recordingDatabase{execErr: &pgconn.PgError{Code: "23505"}}
	response := documentPIIPost(t, database, true, `{"kind":"document_pii","config_json":{"mode":"redact"},"enabled":true}`)
	if response.Code != http.StatusConflict {
		t.Fatalf("status = %d, body=%s, want 409 for a second row", response.Code, response.Body.String())
	}
}

func TestSafetyDocumentPIIEffectiveRouteAnswersTheDefaultWhenNoRowExists(t *testing.T) {
	database := &recordingDatabase{}
	safety := NewSafetyHandler(nil).WithDocumentPIIPolicy(true)
	safety.pool = database
	mux := http.NewServeMux()
	safety.Register(mux)
	handler, readToken, _ := mcpAuthenticatedHandler(t, mux)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/safety/document-pii/effective", nil)
	request.Header.Set("Authorization", "Bearer "+readToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	var got documentPIIEffective
	if err := json.Unmarshal(response.Body.Bytes(), &got); err != nil || got != (documentPIIEffective{Mode: "off", Source: "default"}) {
		t.Fatalf("effective = %+v (%v), body=%s", got, err, response.Body.String())
	}
	// A write method on the same path is not a route.
	post := httptest.NewRequest(http.MethodPost, "/api/v1/safety/document-pii/effective", nil)
	post.Header.Set("Authorization", "Bearer "+readToken)
	notFound := httptest.NewRecorder()
	handler.ServeHTTP(notFound, post)
	if notFound.Code == http.StatusOK || notFound.Code == http.StatusCreated {
		t.Fatalf("POST to the effective route answered %d", notFound.Code)
	}
}
