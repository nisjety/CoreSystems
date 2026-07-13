package events

import (
	"encoding/json"
	"testing"
)

// Cross-language schema lock for dataplane.wiki.version.published.
// Source of truth: docs/schemas/wiki_events.md. The Rust counterpart
// lives at services/embedding-engine-rs/tests/wiki_event_schema.rs.
// If you change the producer struct without updating the canonical
// JSON here (and the Rust side), CI fails here.

const canonicalPayload = `{
  "page_id": "page-1",
  "version_id": "ver-1",
  "org_id": "org-1",
  "workspace_id": "ws-1",
  "title": "Onboarding",
  "path": "/handbook/onboarding",
  "content": "Welcome to the team.",
  "user_id": "user-1",
  "zdr": false
}`

func TestWikiVersionPublishedRoundTrip(t *testing.T) {
	var evt WikiVersionPublishedEvent
	if err := json.Unmarshal([]byte(canonicalPayload), &evt); err != nil {
		t.Fatalf("canonical JSON must parse: %v", err)
	}
	if evt.PageID != "page-1" || evt.VersionID != "ver-1" {
		t.Fatalf("ids mismatch: %+v", evt)
	}
	if evt.OrgID != "org-1" || evt.WorkspaceID != "ws-1" {
		t.Fatalf("scope mismatch: %+v", evt)
	}
	if evt.Title != "Onboarding" || evt.Path != "/handbook/onboarding" {
		t.Fatalf("display fields mismatch: %+v", evt)
	}
	if evt.Content != "Welcome to the team." {
		t.Fatalf("content mismatch: %q", evt.Content)
	}
	if evt.UserID != "user-1" || evt.ZDR {
		t.Fatalf("security posture mismatch: %+v", evt)
	}

	// Producer round-trip: marshaling and re-parsing must be lossless.
	bytes, err := json.Marshal(evt)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var back WikiVersionPublishedEvent
	if err := json.Unmarshal(bytes, &back); err != nil {
		t.Fatalf("re-parse: %v", err)
	}
	if back != evt {
		t.Fatalf("round-trip mismatch: %+v vs %+v", evt, back)
	}
}

func TestWikiVersionPublishedUnknownFieldsIgnored(t *testing.T) {
	withExtra := `{
        "page_id": "p", "version_id": "v", "org_id": "o",
        "workspace_id": "w", "title": "t", "path": "/p", "content": "c", "zdr": false,
        "future_field": "ignored"
    }`
	var evt WikiVersionPublishedEvent
	if err := json.Unmarshal([]byte(withExtra), &evt); err != nil {
		t.Fatalf("must accept unknown fields: %v", err)
	}
	if evt.Title != "t" {
		t.Fatalf("title: got %q", evt.Title)
	}
}
