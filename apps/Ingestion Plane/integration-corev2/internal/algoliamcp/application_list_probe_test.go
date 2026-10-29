package algoliamcp

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
)

func TestFixedApplicationListProbeRequiresEmptyArgumentProof(t *testing.T) {
	for _, valid := range []bool{false, true} {
		f := &metaFixture{listSchema: unenumeratedApplicationSchema, probePayload: `{"tools":[]}`, applicationListPayload: `{"applications":[{"applicationId":"APP1234567","name":"private-name","apiKey":"private-token"}]}`}
		if valid {
			f.applicationListSchema = `{"type":"object","properties":{},"required":[]}`
		} else {
			f.applicationListSchema = `{"type":"object","properties":{},"required":["userIntent"]}`
		}
		s := metaService(t, f)
		var events []Diagnostic
		s.diagnostic = func(d Diagnostic) { events = append(events, d) }
		items, err := s.Applications(context.Background(), "org-1", "conn_one")
		if (err == nil) != valid || (len(items) == 1) != valid {
			t.Fatal("application metadata read ignored empty-argument proof")
		}
		want := 0
		if valid {
			want = 1
		}
		if f.readCalls != want {
			t.Fatalf("unproved app-list dispatch reads=%d", f.readCalls)
		}
		raw, _ := json.Marshal(events)
		if !valid && !strings.Contains(string(raw), "application-list-probe") {
			t.Fatal("missing app-list phase")
		}
		if valid && len(events) != 0 {
			t.Fatal("successful application metadata logged")
		}
		for _, secret := range []string{"APP1234567", "private-name", "private-token", "apiKey"} {
			if strings.Contains(string(raw), secret) {
				t.Fatal("app metadata values leaked")
			}
		}
	}
}

func TestApplicationListEmptyArgsRejectsMalformedRootConstraints(t *testing.T) {
	for _, raw := range []string{`{"type":"object","required":null}`, `{"type":"object","properties":null}`, `{"type":"object","additionalProperties":null}`, `{"type":"object","additionalProperties":"private"}`, `{"type":"object","additionalProperties":{"$ref":"#private"}}`} {
		if verifiedEmptyArguments(schemaTool(raw)) {
			t.Fatalf("malformed empty schema accepted %s", raw)
		}
	}
	for _, raw := range []string{`{"type":"object"}`, `{"type":"object","properties":{},"required":[],"additionalProperties":false}`, `{"type":"object","additionalProperties":true}`, `{"type":"object","additionalProperties":{}}`} {
		if !verifiedEmptyArguments(schemaTool(raw)) {
			t.Fatalf("valid empty schema rejected %s", raw)
		}
	}
}

func TestApplicationListCannotUseUnprovedDirectToolShortcut(t *testing.T) {
	f := &metaFixture{catalog: `{"tools":[{"name":"algolia_application_list","inputSchema":{"type":"object","properties":{},"required":[]}}]}`}
	s := metaService(t, f)
	session, tools, done, err := s.open(context.Background(), "org-1", "conn_one")
	if err != nil {
		t.Fatal(err)
	}
	defer done()
	if _, err = session.resolve(context.Background(), tools, applicationList); err == nil {
		t.Fatal("unproved direct app-list schema accepted")
	}
	if _, err = session.call(context.Background(), applicationList, map[string]any{}); err == nil || f.readCalls != 0 {
		t.Fatal("direct app-list shortcut dispatched")
	}
}
