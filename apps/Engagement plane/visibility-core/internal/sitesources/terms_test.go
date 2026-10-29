package sitesources

import (
	"encoding/json"
	"strings"
	"testing"
)

func termsReceipt(t *testing.T, top, missing string, analytics bool) []byte {
	t.Helper()
	_, _, c, _ := fixture(t)
	raw, _ := json.Marshal(c.report)
	var report map[string]any
	_ = json.Unmarshal(raw, &report)
	a := report["algolia"].(map[string]any)
	if !analytics {
		a["analytics"] = map[string]any{"status": "unavailable", "reportedDays": 0}
	}
	a["topSearches"] = json.RawMessage(top)
	a["noResultTerms"] = json.RawMessage(missing)
	raw, _ = json.Marshal(map[string]any{"success": true, "data": report})
	return raw
}

func TestIndependentAlgoliaTermsPreserveActualEmptyAndMissing(t *testing.T) {
	for _, top := range []string{`{"status":"connected","limit":20,"rows":[]}`, `{"status":"connected","limit":20,"rows":[{"query":"","count":0,"averageResults":0}]}`} {
		raw := termsReceipt(t, top, `{"status":"unavailable","limit":20}`, false)
		if _, err := DecodeReceipt(raw); err != nil {
			t.Fatal("real connected term observations rejected", err)
		}
	}
}

func TestAlgoliaTermsRejectMalformedAndUnboundedReceipts(t *testing.T) {
	for _, terms := range []string{
		`{"status":"connected","limit":20}`, `{"status":"connected","limit":20,"rows":null}`,
		`{"status":"unavailable","limit":20,"rows":[]}`, `{"status":"connected","limit":21,"rows":[]}`,
		`{"status":"connected","limit":20,"rows":[{"query":"x","count":-1}]}`,
		`{"status":"connected","limit":20,"rows":[{"query":"x","count":9007199254740992}]}`,
		`{"status":"connected","limit":20,"rows":[{"query":"x","count":1,"averageResults":null}]}`,
		`{"status":"connected","limit":20,"rows":[{"query":"x","count":1,"averageResults":-1}]}`,
		`{"status":"connected","limit":20,"rows":[{"query":"x","count":1},{"query":"x","count":2}]}`,
		`{"status":"connected","limit":20,"rows":[{"query":"x\u0000","count":1}]}`,
		`{"status":"connected","limit":20,"rows":[{"query":"x\u0085","count":1}]}`,
	} {
		if _, err := DecodeReceipt(termsReceipt(t, terms, `{"status":"unavailable","limit":20}`, true)); err == nil {
			t.Fatal("unsafe terms accepted", terms)
		}
	}
	for _, query := range []string{strings.Repeat("a", 513), strings.Repeat("🙂", 513)} {
		component, _ := json.Marshal(map[string]any{"status": "connected", "limit": 20, "rows": []any{map[string]any{"query": query, "count": 1}}})
		if _, err := DecodeReceipt(termsReceipt(t, string(component), `{"status":"unavailable","limit":20}`, true)); err == nil {
			t.Fatal("oversized term accepted")
		}
	}
	valid, _ := json.Marshal(map[string]any{"status": "connected", "limit": 20, "rows": []any{map[string]any{"query": strings.Repeat("🙂", 512), "count": 1}}})
	if _, err := DecodeReceipt(termsReceipt(t, string(valid), `{"status":"unavailable","limit":20}`, true)); err != nil {
		t.Fatal("bounded Unicode term rejected", err)
	}
}
