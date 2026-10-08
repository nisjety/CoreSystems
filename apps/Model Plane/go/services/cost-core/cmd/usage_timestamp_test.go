package main

import (
	"testing"
	"time"
)

func TestDurableUsageKeepsSourceTimeAndRejectsZDR(t *testing.T) {
	body := []byte(`{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","org_id":"org-a","user_id":"user-a","idempotency_key":"req-a-USAGE_ENVELOPE","ts":"2026-09-01T12:00:00Z","payload":{"request_id":"req-a","input_tokens":12,"cost_basis":"subscription"}}`)
	e, err := decodeUsageMessage("mp.v1.usage.org-a", body, time.Now().UTC())
	if err != nil {
		t.Fatal(err)
	}
	if e.CreatedAt.Format(time.RFC3339) != "2026-09-01T12:00:00Z" {
		t.Fatalf("replay moved historical usage: %s", e.CreatedAt)
	}
	zdr := []byte(`{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","org_id":"org-a","user_id":"user-a","idempotency_key":"req-a-USAGE_ENVELOPE","zdr":true}`)
	if _, err := decodeUsageMessage("mp.v1.usage.org-a", zdr, time.Now()); err == nil {
		t.Fatal("ZDR envelope persisted")
	}
}
