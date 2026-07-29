package main

import (
	"context"
	"strings"
	"testing"
	"time"
)

func TestDurableLedgerIsRequiredUnlessEphemeralModeIsExplicit(t *testing.T) {
	if _, err := buildLedgerFromConfig(context.Background(), "", false); err == nil {
		t.Fatal("missing database must fail closed")
	}
	store, err := buildLedgerFromConfig(context.Background(), "", true)
	if err != nil || store == nil {
		t.Fatalf("explicit ephemeral mode should be available for tests: store=%v err=%v", store, err)
	}
}

func TestUsageEnvelopeMustMatchAuthenticatedSubjectScope(t *testing.T) {
	now := time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)
	valid := []byte(`{
		"event_type":"USAGE_ENVELOPE",
		"producer":"model-gateway",
		"correlation_id":"request-a",
		"idempotency_key":"usage-a",
		"org_id":"org-a",
		"user_id":"user-a",
		"payload":{"request_id":"request-a","org_id":"org-a","user_id":"user-a","model":"test","input_tokens":3}
	}`)
	entry, err := decodeUsageMessage("mp.v1.usage.org-a", valid, now)
	if err != nil {
		t.Fatalf("valid envelope rejected: %v", err)
	}
	if entry.OrgID != "org-a" || entry.UserID != "user-a" || entry.ProducerID != "model-gateway" || entry.CreatedAt != now {
		t.Fatalf("unexpected entry: %+v", entry)
	}

	tests := []struct {
		name    string
		subject string
		data    string
	}{
		{name: "subject tenant mismatch", subject: "mp.v1.usage.org-b", data: string(valid)},
		{name: "payload tenant mismatch", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"org_id":"org-b"}}`},
		{name: "payload user mismatch", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"user_id":"user-b"}}`},
		{name: "missing event type", subject: "mp.v1.usage.org-a", data: `{"idempotency_key":"x","org_id":"org-a"}`},
		{name: "missing idempotency", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","org_id":"org-a"}`},
		{name: "invalid subject", subject: "mp.v1.usage.org-a.extra", data: string(valid)},
		{name: "missing producer attribution", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","idempotency_key":"x","org_id":"org-a","user_id":"user-a"}`},
		{name: "forged producer attribution", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"untrusted-service","idempotency_key":"x","org_id":"org-a","user_id":"user-a"}`},
		{name: "missing user attribution", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a"}`},
		{name: "negative input", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"input_tokens":-1}}`},
		{name: "negative output", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"output_tokens":-1}}`},
		{name: "overflow token", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"input_tokens":1000000000001}}`},
		{name: "negative cost", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"cost_usd":-1}}`},
		{name: "overflow cost", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"cost_usd":1e1000}}`},
		{name: "oversized model", subject: "mp.v1.usage.org-a", data: `{"event_type":"USAGE_ENVELOPE","producer":"model-gateway","idempotency_key":"x","org_id":"org-a","user_id":"user-a","payload":{"model":"` + strings.Repeat("m", 257) + `"}}`},
		{name: "oversized envelope", subject: "mp.v1.usage.org-a", data: strings.Repeat("x", maxUsageMessageBytes+1)},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if _, err := decodeUsageMessage(test.subject, []byte(test.data), now); err == nil {
				t.Fatal("expected rejection")
			}
		})
	}
}

// The budget gate fix: inference-core forwards the caller's aud=inference-core
// token, so COST_CORE_AUTH_AUDIENCE must parse as a CSV list. A single value
// (the historical deployment) must keep working unchanged.
func TestSplitAudiencesParsesSingleAndCSVValues(t *testing.T) {
	tests := []struct {
		name string
		raw  string
		want []string
	}{
		{name: "single value unchanged", raw: "cost-core", want: []string{"cost-core"}},
		{name: "two values", raw: "cost-core,inference-core", want: []string{"cost-core", "inference-core"}},
		{name: "whitespace trimmed", raw: " cost-core , inference-core ", want: []string{"cost-core", "inference-core"}},
		{name: "empty entries dropped", raw: "cost-core,,inference-core,", want: []string{"cost-core", "inference-core"}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got := splitAudiences(test.raw)
			if len(got) != len(test.want) {
				t.Fatalf("got %v, want %v", got, test.want)
			}
			for i := range got {
				if got[i] != test.want[i] {
					t.Fatalf("got %v, want %v", got, test.want)
				}
			}
		})
	}
}
