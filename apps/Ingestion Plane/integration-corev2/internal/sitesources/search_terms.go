package sitesources

import (
	"bytes"
	"encoding/json"
	"math"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/integration-corev2/internal/webvitals"
)

func UnavailableSearchTerms() *SearchTerms { return &SearchTerms{Status: "unavailable", Limit: 20} }

// ParseAlgoliaSearchTerms accepts the documented Analytics searches envelope.
// It projects only a bounded user-authorized shortlist, never user identifiers,
// keys, hit records or unrelated source metadata. It never logs term values.
func ParseAlgoliaSearchTerms(raw []byte) SearchTerms {
	failed := *UnavailableSearchTerms()
	if len(raw) > 2<<20 || !utf8.Valid(raw) || webvitals.UniqueJSON(raw) != nil {
		return failed
	}
	root, ok := object(raw)
	if !ok || len(root) != 1 {
		return failed
	}
	var source []json.RawMessage
	if json.Unmarshal(root["searches"], &source) != nil || source == nil || len(source) > 20 {
		return failed
	}
	rows := make([]SearchTermRow, 0, len(source))
	seen := map[string]bool{}
	for _, rawRow := range source {
		row, ok := parseSearchTerm(rawRow)
		if !ok || seen[row.Query] {
			return failed
		}
		seen[row.Query] = true
		rows = append(rows, row)
	}
	return SearchTerms{Status: "connected", Limit: 20, Rows: &rows}
}
func parseSearchTerm(raw json.RawMessage) (SearchTermRow, bool) {
	fields, ok := object(raw)
	if !ok {
		return SearchTermRow{}, false
	}
	for _, key := range []string{"query", "averageResults", "error", "errors", "isError"} {
		if _, exists := fields[key]; exists {
			return SearchTermRow{}, false
		}
	}
	query, ok := text(fields, "search")
	if !ok || !utf8.ValidString(query) || len(query) > 2048 || utf8.RuneCountInString(query) > 512 {
		return SearchTermRow{}, false
	}
	for _, r := range query {
		if unicode.IsControl(r) || r == utf8.RuneError {
			return SearchTermRow{}, false
		}
	}
	n, ok := count(fields["count"])
	if !ok {
		return SearchTermRow{}, false
	}
	row := SearchTermRow{Query: query, Count: n}
	if value, exists := fields["nbHits"]; exists {
		var average float64
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) || json.Unmarshal(value, &average) != nil || math.IsNaN(average) || math.IsInf(average, 0) || average < 0 || average > float64(safeMax) {
			return SearchTermRow{}, false
		}
		row.AverageResults = &average
	}
	return row, true
}
