package sitesources

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestAlgoliaSearchTermsNormalizeOnlyBoundedProviderObservations(t *testing.T) {
	for _, tc := range []struct {
		raw   string
		valid bool
		rows  int
	}{
		{`{"searches":[{"search":"","count":0},{"search":"<script>Norwegian æøå</script>","count":7,"nbHits":1.5}]}`, true, 2},
		{`{"searches":[]}`, true, 0}, {`{}`, false, 0}, {`{"searches":null}`, false, 0}, {`{"queries":[]}`, false, 0},
		{`{"searches":[{"search":"private\nterm","count":1}]}`, false, 0}, {`{"searches":[{"search":"private\u0085term","count":1}]}`, false, 0},
		{`{"searches":[{"search":"x","count":-1}]}`, false, 0}, {`{"searches":[{"search":"x","count":1.5}]}`, false, 0}, {`{"searches":[{"search":"x","count":9007199254740992}]}`, false, 0},
		{`{"searches":[{"search":"x","count":1,"nbHits":null}]}`, false, 0}, {`{"searches":[{"search":"x","count":1,"nbHits":-1}]}`, false, 0},
		{`{"searches":[{"search":"x","count":1,"nbHits":9007199254740992}]}`, false, 0},
		{`{"searches":[{"search":"x","count":1},{"search":"x","count":1}]}`, false, 0},
		{`{"searches":[],"nextCursor":"private"}`, false, 0}, {`{"searches":[],"error":"private"}`, false, 0}, {`{"searches":[],"searches":[]}`, false, 0},
		{`{"searches":[{"search":"x","query":"other","count":1}]}`, false, 0},
	} {
		got := ParseAlgoliaSearchTerms([]byte(tc.raw))
		if (got.Status == "connected") != tc.valid || got.Limit != 20 {
			t.Fatalf("bad term receipt %s %+v", tc.raw, got)
		}
		if tc.valid && (got.Rows == nil || len(*got.Rows) != tc.rows) {
			t.Fatal("explicit observed list lost")
		}
		encoded, _ := json.Marshal(got)
		if !tc.valid && (got.Rows != nil || strings.Contains(string(encoded), `"rows"`)) {
			t.Fatal("unavailable terms fabricated rows")
		}
		if tc.valid && tc.rows == 0 && !strings.Contains(string(encoded), `"rows":[]`) {
			t.Fatal("explicit empty not represented")
		}
	}
}

func TestAlgoliaTermBoundsAndPrivacyProjection(t *testing.T) {
	for _, query := range []string{strings.Repeat("a", 513), strings.Repeat("界", 513), string([]byte{0xff}), "\x00", "\x7f"} {
		raw, _ := json.Marshal(map[string]any{"searches": []any{map[string]any{"search": query, "count": 1}}})
		if ParseAlgoliaSearchTerms(raw).Status == "connected" {
			t.Fatal("unsafe term accepted")
		}
	}
	rows := make([]any, 21)
	for i := range rows {
		rows[i] = map[string]any{"search": "x", "count": 1}
	}
	raw, _ := json.Marshal(map[string]any{"searches": rows})
	if ParseAlgoliaSearchTerms(raw).Status == "connected" {
		t.Fatal("unbounded shortlist")
	}
	valid := ParseAlgoliaSearchTerms([]byte(`{"searches":[{"search":" preserve  ","count":2,"nbHits":3,"userId":"private-user","apiKey":"private-key"}]}`))
	encoded, _ := json.Marshal(valid)
	if valid.Status != "connected" || (*valid.Rows)[0].Query != " preserve  " || strings.Contains(string(encoded), "private-") {
		t.Fatal("query mutated or metadata escaped")
	}
}
