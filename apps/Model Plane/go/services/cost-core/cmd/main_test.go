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
		"payload":{"request_id":"request-a","org_id":"org-a","user_id":"user-a","model":"test","provider_used":"azure-norway-eu","input_tokens":3}
	}`)
	entry, err := decodeUsageMessage("mp.v1.usage.org-a", valid, now)
	if err != nil {
		t.Fatalf("valid envelope rejected: %v", err)
	}
	if entry.OrgID != "org-a" || entry.UserID != "user-a" || entry.ProducerID != "model-gateway" || entry.CreatedAt != now {
		t.Fatalf("unexpected entry: %+v", entry)
	}
	if entry.ProviderUsed != "azure-norway-eu" {
		t.Fatalf("provider attribution not decoded: %+v", entry)
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

// The gateway's advisory cache miss reason rides the usage payload into the
// ledger entry; an unknown value is dropped, never a reason to lose the tokens.
func TestUsageEnvelopeDecodesCacheMissReason(t *testing.T) {
	now := time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)
	envelopeWith := func(requestID, field string) []byte {
		return []byte(`{
			"event_type":"USAGE_ENVELOPE",
			"producer":"model-gateway",
			"correlation_id":"` + requestID + `",
			"idempotency_key":"` + requestID + `-USAGE_ENVELOPE",
			"org_id":"org-a",
			"user_id":"user-a",
			"payload":{"request_id":"` + requestID + `","org_id":"org-a","user_id":"user-a","model":"claude-sonnet-4-6","input_tokens":5000,"output_tokens":40` + field + `}
		}`)
	}
	for name, test := range map[string]struct{ field, want string }{
		"classified": {`,"cache_miss_reason":"prefix_changed"`, ledger.CacheMissPrefixChanged},
		"older":      {``, ""},
		"future":     {`,"cache_miss_reason":"cold_start"`, ""},
	} {
		entry, err := decodeUsageMessage("mp.v1.usage.org-a", envelopeWith("req-"+name, test.field), now)
		if err != nil {
			t.Fatalf("%s: valid envelope rejected: %v", name, err)
		}
		if entry.CacheMissReason != test.want || entry.InputTokens != 5000 {
			t.Fatalf("%s: reason=%q input=%d, want %q/5000", name, entry.CacheMissReason, entry.InputTokens, test.want)
		}
	}
}

// WS16.S1: the gateway meters a machine caller of POST /v1/ai/chat under the org, with the
// service subject as the user. This is the envelope model-gateway's ai_chat lane publishes for
// service:quarry-edge (route and purpose ride beside the usual counters). It must decode, pass the
// subject-scope checks, and land on the org's ledger with its real tokens and a price.
func TestUsageEnvelopeDecodesAiChatServiceEnvelope(t *testing.T) {
	now := time.Date(2026, 11, 1, 12, 0, 0, 0, time.UTC)
	envelope := []byte(`{
		"event_type":"USAGE_ENVELOPE",
		"schema_version":1,
		"producer":"model-gateway",
		"correlation_id":"01J0000000000000000000AICH",
		"idempotency_key":"01J0000000000000000000AICH-USAGE_ENVELOPE",
		"org_id":"org-a",
		"user_id":"service:quarry-edge",
		"resource_ref":"request/01J0000000000000000000AICH",
		"payload":{
			"request_id":"01J0000000000000000000AICH","org_id":"org-a","user_id":"service:quarry-edge",
			"model":"claude-sonnet-4-6","input_tokens":412,"output_tokens":23,"latency_ms":840,
			"transport":"http","route":"ai_chat","purpose":"quarry.extract",
			"provider_used":"azure","residency":"",
			"cache_read_input_tokens":0,"cache_creation_input_tokens":0,
			"cost_basis":"metered"
		}
	}`)
	entry, err := decodeUsageMessage("mp.v1.usage.org-a", envelope, now)
	if err != nil {
		t.Fatalf("the ai_chat service envelope was rejected: %v", err)
	}
	if entry.OrgID != "org-a" || entry.UserID != "service:quarry-edge" || entry.ProducerID != "model-gateway" {
		t.Fatalf("attribution not decoded: %+v", entry)
	}
	if entry.InputTokens != 412 || entry.OutputTokens != 23 || entry.ProviderUsed != "azure" {
		t.Fatalf("counters or provider not decoded: %+v", entry)
	}
	if entry.CostBasis != ledger.CostBasisMetered {
		t.Fatalf("cost basis = %q, want %q", entry.CostBasis, ledger.CostBasisMetered)
	}

	store := ledger.NewStore()
	srv := server.NewServer(store)
	if err := srv.RecordUsage(context.Background(), entry); err != nil {
		t.Fatalf("RecordUsage: %v", err)
	}
	entries, err := store.ListEntries(context.Background(), ledger.AggregateFilter{OrgID: "org-a"}, 10)
	if err != nil || len(entries) != 1 {
		t.Fatalf("entries = %d (err=%v), want exactly 1 on the org's ledger", len(entries), err)
	}
	if entries[0].UserID != "service:quarry-edge" || entries[0].InputTokens != 412 {
		t.Fatalf("ledger row: %+v", entries[0])
	}
	if entries[0].CostUSD <= 0 {
		t.Fatalf("a metered service call must be priced, got %.8f", entries[0].CostUSD)
	}

	// The subject-scope rules hold for a service subject too: another org's subject, or a payload
	// that names a different user than the envelope, is refused.
	if _, err := decodeUsageMessage("mp.v1.usage.org-b", envelope, now); err == nil {
		t.Fatal("an envelope for org-a on org-b's subject must be rejected")
	}
	mismatched := strings.Replace(string(envelope), `"org_id":"org-a","user_id":"service:quarry-edge",`, `"org_id":"org-a","user_id":"service:other",`, 1)
	if mismatched == string(envelope) {
		t.Fatal("test setup: the payload user was not rewritten")
	}
	if _, err := decodeUsageMessage("mp.v1.usage.org-a", []byte(mismatched), now); err == nil {
		t.Fatal("a payload user that differs from the envelope user must be rejected")
	}
}
