package main

import (
	"encoding/json"
	"os"
	"regexp"
	"sort"
	"strings"
	"testing"
	"time"
)

// FU-30: cost-core admits a zdr=true usage envelope only as the explicit ZDR_SPEND kind carrying
// the allowlisted fields, and stores it as a content-free row marked zdr. Every other zdr=true
// usage is still refused.

func zdrSpendPayloadJSON(extra map[string]any, drop ...string) map[string]any {
	payload := map[string]any{
		"request_id": "req-z", "org_id": "org-a", "user_id": "user-a", "model": "claude-sonnet-5-5",
		"provider_used": "anthropic", "input_tokens": 120, "output_tokens": 30,
		"cache_read_input_tokens": 0, "cache_creation_input_tokens": 0,
		"cost_usd": 0.0042, "currency": "USD", "cost_basis": "metered", "retention_class": "zdr",
	}
	for _, key := range drop {
		delete(payload, key)
	}
	for key, value := range extra {
		payload[key] = value
	}
	return payload
}

func zdrSpendEnvelope(t *testing.T, eventType string, zdr bool, payload map[string]any) []byte {
	t.Helper()
	raw, err := json.Marshal(map[string]any{
		"event_type": eventType, "producer": "model-gateway", "correlation_id": "req-z",
		"idempotency_key": "req-z-ZDR_SPEND", "org_id": "org-a", "user_id": "user-a", "zdr": zdr,
		"payload": payload,
	})
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestZDRSpendRowIsAdmittedAndMarked(t *testing.T) {
	now := time.Date(2026, 11, 12, 9, 0, 0, 0, time.UTC)
	for _, retention := range []string{"zdr", "zdr_waived"} {
		entry, err := decodeUsageMessage("mp.v1.usage.org-a",
			zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"retention_class": retention})), now)
		if err != nil {
			t.Fatalf("%s: a valid spend row was refused: %v", retention, err)
		}
		if !entry.ZDR || entry.OrgID != "org-a" || entry.UserID != "user-a" || entry.RequestID != "req-z" ||
			entry.Model != "claude-sonnet-5-5" || entry.InputTokens != 120 || entry.OutputTokens != 30 ||
			entry.CostUSD != 0.0042 || entry.CostBasis != "metered" {
			t.Fatalf("%s: entry = %+v", retention, entry)
		}
		// A spend row names no run and no cache-miss reason, and is never priced on the priority tier.
		if entry.RunID != "" || entry.CacheMissReason != "" || entry.PriorityTier {
			t.Fatalf("%s: spend row carries more than its allowlist: %+v", retention, entry)
		}
	}
	// The optional fields may be absent.
	if _, err := decodeUsageMessage("mp.v1.usage.org-a", zdrSpendEnvelope(t, "ZDR_SPEND", true,
		zdrSpendPayloadJSON(nil, "cost_usd", "cost_basis", "cache_read_input_tokens", "cache_creation_input_tokens")), now); err != nil {
		t.Fatalf("a spend row without its optional fields was refused: %v", err)
	}
}

func TestEveryOtherZDRUsageIsStillRefused(t *testing.T) {
	now := time.Date(2026, 11, 12, 9, 0, 0, 0, time.UTC)
	cases := map[string][]byte{
		"usage envelope with zdr":           zdrSpendEnvelope(t, "USAGE_ENVELOPE", true, zdrSpendPayloadJSON(nil)),
		"usage envelope naming the kind":    zdrSpendEnvelope(t, "USAGE_ENVELOPE", true, zdrSpendPayloadJSON(map[string]any{"kind": "zdr_spend"})),
		"lower-case kind":                   zdrSpendEnvelope(t, "zdr_spend", true, zdrSpendPayloadJSON(nil)),
		"spend row that does not claim zdr": zdrSpendEnvelope(t, "ZDR_SPEND", false, zdrSpendPayloadJSON(nil)),
		"thread id":                         zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"thread_id": "thr-1"})),
		"run id":                            zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"run_id": "run-1"})),
		"message text":                      zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"text": "the question"})),
		"tool name":                         zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"tool_name": "web_fetch"})),
		"cache miss reason":                 zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"cache_miss_reason": "ttl_expired"})),
		"service tier":                      zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"service_tier_used": "priority"})),
		"case variant of an allowed key":    zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"Request_ID": "req-z"})),
		"missing model":                     zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(nil, "model")),
		"missing retention class":           zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(nil, "retention_class")),
		"persistent retention class":        zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"retention_class": "persistent"})),
		"another currency":                  zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"currency": "EUR"})),
		"unknown cost basis":                zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"cost_basis": "free"})),
		"cost basis not canonical":          zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"cost_basis": "Metered"})),
		"token count as text":               zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"input_tokens": "120"})),
		"control character in an id":        zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"model": "m\nx"})),
		"payload org mismatch":              zdrSpendEnvelope(t, "ZDR_SPEND", true, zdrSpendPayloadJSON(map[string]any{"org_id": "org-b"})),
		"payload not an object":             []byte(`{"event_type":"ZDR_SPEND","producer":"model-gateway","idempotency_key":"k","org_id":"org-a","user_id":"user-a","zdr":true,"payload":[1]}`),
		"no payload":                        []byte(`{"event_type":"ZDR_SPEND","producer":"model-gateway","idempotency_key":"k","org_id":"org-a","user_id":"user-a","zdr":true}`),
	}
	for name, body := range cases {
		t.Run(name, func(t *testing.T) {
			if entry, err := decodeUsageMessage("mp.v1.usage.org-a", body, now); err == nil {
				t.Fatalf("admitted: %+v", entry)
			}
		})
	}
}

// The ordinary usage path is unchanged: zdr=false usage is stored unmarked.
func TestOrdinaryUsageIsNotMarkedZDR(t *testing.T) {
	entry, err := decodeUsageMessage("mp.v1.usage.org-a", zdrSpendEnvelope(t, "USAGE_ENVELOPE", false, map[string]any{
		"request_id": "req-z", "org_id": "org-a", "user_id": "user-a", "model": "m", "input_tokens": 1,
	}), time.Now())
	if err != nil || entry.ZDR {
		t.Fatalf("entry=%+v err=%v", entry, err)
	}
}

// The Go allowlist mirrors mp-contracts zdr_spend.rs exactly (its module doc: a change to one is a
// change to both). The publisher and the owner must agree, or a row is published and refused, or
// a field the owner never agreed to is stored.
func TestZDRSpendAllowlistMatchesMPContracts(t *testing.T) {
	raw, err := os.ReadFile("../../../../rust/crates/mp-contracts/src/zdr_spend.rs")
	if err != nil {
		t.Fatalf("read zdr_spend.rs: %v", err)
	}
	src := strings.ReplaceAll(string(raw), "\r\n", "\n")
	list := func(name string) []string {
		m := regexp.MustCompile(`(?s)pub const ` + name + `: &\[&str\] = &\[(.*?)\];`).FindStringSubmatch(src)
		if m == nil {
			t.Fatalf("zdr_spend.rs has no %s", name)
		}
		var out []string
		for _, q := range regexp.MustCompile(`"([^"]+)"`).FindAllStringSubmatch(m[1], -1) {
			out = append(out, q[1])
		}
		sort.Strings(out)
		return out
	}
	goFields := make([]string, 0, len(zdrSpendPayloadFields))
	for key := range zdrSpendPayloadFields {
		goFields = append(goFields, key)
	}
	sort.Strings(goFields)
	if got, want := strings.Join(goFields, ","), strings.Join(list("PAYLOAD_FIELDS"), ","); got != want {
		t.Errorf("payload fields differ:\n go   %s\n rust %s", got, want)
	}
	goRequired := append([]string(nil), zdrSpendRequiredFields...)
	sort.Strings(goRequired)
	if got, want := strings.Join(goRequired, ","), strings.Join(list("REQUIRED_FIELDS"), ","); got != want {
		t.Errorf("required fields differ:\n go   %s\n rust %s", got, want)
	}
	if !strings.Contains(src, `pub const EVENT_TYPE: &str = "`+zdrSpendEventType+`";`) {
		t.Error("event type differs from mp-contracts")
	}
	if !strings.Contains(src, `pub const CURRENCY: &str = "`+zdrSpendCurrency+`";`) {
		t.Error("currency differs from mp-contracts")
	}
	if got := strings.Join(list("RETENTION_CLASSES"), ","); got != "zdr,zdr_waived" {
		t.Errorf("retention classes differ: %s", got)
	}
}
