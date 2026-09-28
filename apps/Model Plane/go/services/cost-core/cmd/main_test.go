package main

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
	"github.com/triodelab/model-plane/services/cost-core/internal/server"
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

// Cache-token telemetry (native-compaction migration prerequisite): the two
// new payload fields decode onto the ledger entry so cache-hit rate and its
// cost savings are queryable off the durable ledger, and an older gateway
// build that omits the keys still decodes cleanly to exact zeros.
func TestUsageEnvelopeDecodesCacheTokenTelemetry(t *testing.T) {
	now := time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)
	withCache := []byte(`{
		"event_type":"USAGE_ENVELOPE",
		"producer":"model-gateway",
		"correlation_id":"request-cache",
		"idempotency_key":"usage-cache",
		"org_id":"org-a",
		"user_id":"user-a",
		"payload":{
			"request_id":"request-cache","org_id":"org-a","user_id":"user-a","model":"claude-sonnet-4-6",
			"input_tokens":8520,"output_tokens":42,
			"cache_read_input_tokens":8000,"cache_creation_input_tokens":400
		}
	}`)
	entry, err := decodeUsageMessage("mp.v1.usage.org-a", withCache, now)
	if err != nil {
		t.Fatalf("valid envelope with cache telemetry rejected: %v", err)
	}
	if entry.CacheReadInputTokens != 8_000 || entry.CacheCreationInputTokens != 400 {
		t.Fatalf("cache token telemetry not decoded: %+v", entry)
	}

	// An older gateway build (or an uncached turn) omits the keys entirely;
	// they must decode to exact zeros, never fail or leave stale state.
	noCache := []byte(`{
		"event_type":"USAGE_ENVELOPE",
		"producer":"model-gateway",
		"correlation_id":"request-nocache",
		"idempotency_key":"usage-nocache",
		"org_id":"org-a",
		"user_id":"user-a",
		"payload":{"request_id":"request-nocache","org_id":"org-a","user_id":"user-a","model":"gpt-4o","input_tokens":10,"output_tokens":5}
	}`)
	entry, err = decodeUsageMessage("mp.v1.usage.org-a", noCache, now)
	if err != nil {
		t.Fatalf("valid envelope without cache telemetry rejected: %v", err)
	}
	if entry.CacheReadInputTokens != 0 || entry.CacheCreationInputTokens != 0 {
		t.Fatalf("cache token telemetry should default to zero, got: %+v", entry)
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

// The gateway stamps `cost_basis` on every USAGE_ENVELOPE. A subscription
// turn must reach the ledger with its real tokens and cost 0; before this
// field was decoded, RecordUsage re-priced it at the `default` list rate
// because gpt-5.6-terra has no catalogue row.
func TestUsageEnvelopeSubscriptionTurnLedgersRealTokensAtZeroCost(t *testing.T) {
	now := time.Date(2026, 9, 24, 12, 0, 0, 0, time.UTC)
	envelopeWith := func(requestID, basis string) []byte {
		basisField := ""
		if basis != "" {
			basisField = `,"cost_basis":"` + basis + `"`
		}
		return []byte(`{
			"event_type":"USAGE_ENVELOPE",
			"producer":"model-gateway",
			"correlation_id":"` + requestID + `",
			"idempotency_key":"` + requestID + `-USAGE_ENVELOPE",
			"org_id":"org-a",
			"user_id":"user-a",
			"payload":{
				"request_id":"` + requestID + `","org_id":"org-a","user_id":"user-a",
				"model":"gpt-5.6-terra","input_tokens":1800,"output_tokens":120,
				"cache_read_input_tokens":1024,"cache_creation_input_tokens":256,
				"provider_used":"openai-codex-subscription","residency":"",
				"timing":{"server_ms":900,"provider_ms":700}` + basisField + `
			}
		}`)
	}

	store := ledger.NewStore()
	srv := server.NewServer(store)
	for requestID, test := range map[string]struct {
		raw  string
		want string
	}{
		"req-sub":     {raw: "subscription", want: ledger.CostBasisSubscription},
		"req-metered": {raw: "metered", want: ledger.CostBasisMetered},
		"req-old":     {raw: "", want: ledger.CostBasisUnknown},
		// An unrecognised basis must not drop the tokens from the ledger.
		"req-future": {raw: "prepaid", want: ledger.CostBasisUnknown},
	} {
		entry, err := decodeUsageMessage("mp.v1.usage.org-a", envelopeWith(requestID, test.raw), now)
		if err != nil {
			t.Fatalf("%s: valid envelope rejected: %v", requestID, err)
		}
		if entry.CostBasis != test.want {
			t.Fatalf("%s: cost basis = %q, want %q", requestID, entry.CostBasis, test.want)
		}
		if err := srv.RecordUsage(context.Background(), entry); err != nil {
			t.Fatalf("%s: RecordUsage: %v", requestID, err)
		}
	}

	entries, err := store.ListEntries(context.Background(), ledger.AggregateFilter{OrgID: "org-a"}, 10)
	if err != nil || len(entries) != 4 {
		t.Fatalf("entries = %d (err=%v), want 4", len(entries), err)
	}
	for _, e := range entries {
		if e.InputTokens != 1_800 || e.OutputTokens != 120 {
			t.Fatalf("%s: tokens = %d/%d, want the real 1800/120", e.RequestID, e.InputTokens, e.OutputTokens)
		}
		switch e.RequestID {
		case "req-sub":
			if e.CostUSD != 0 {
				t.Fatalf("subscription turn ledgered at list price: %.8f", e.CostUSD)
			}
		default:
			if e.CostUSD <= 0 {
				t.Fatalf("%s: a non-subscription turn must still be priced, got %.8f", e.RequestID, e.CostUSD)
			}
		}
	}
}
