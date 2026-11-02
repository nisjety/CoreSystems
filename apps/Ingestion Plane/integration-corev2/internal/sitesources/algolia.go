package sitesources

import (
	"context"
	"encoding/json"
	"net/url"
)

func (s *Service) algolia(ctx context.Context, b algoliaBinding, key string, window Window, stillBound func() bool) *Algolia {
	query := url.Values{"index": {b.IndexName}, "startDate": {window.StartDate}, "endDate": {window.EndDate}}
	raw, status := s.fetch(ctx, "https://analytics."+b.Region+".algolia.com/2/searches/noResultRate?"+query.Encode(), map[string]string{"X-Algolia-Application-Id": b.ApplicationID, "X-Algolia-API-Key": key}, stillBound)
	analytics := Analytics{Status: status}
	if status == "" {
		analytics = parseAnalytics(raw, window)
	}
	return &Algolia{IndexName: b.IndexName, Region: b.Region, Analytics: analytics, TopSearches: UnavailableSearchTerms(), NoResultTerms: UnavailableSearchTerms()}
}
func parseAnalytics(raw []byte, window Window) Analytics {
	failed := Analytics{Status: "unavailable"}
	root, ok := object(raw)
	if !ok {
		return failed
	}
	total, ok := count(root["count"])
	if !ok {
		return failed
	}
	empty, ok := count(root["noResultCount"])
	if !ok || empty > total || !ratio(root["rate"], empty, total) {
		return failed
	}
	var dates []json.RawMessage
	if json.Unmarshal(root["dates"], &dates) != nil || dates == nil || len(dates) > 7 || total > 0 && len(dates) == 0 {
		return failed
	}
	seen := map[string]bool{}
	sum, sumEmpty := int64(0), int64(0)
	for _, daily := range dates {
		fields, ok := object(daily)
		if !ok {
			return failed
		}
		date, ok := text(fields, "date")
		if !ok || !dateInWindow(date, window) || seen[date] {
			return failed
		}
		seen[date] = true
		n, ok := count(fields["count"])
		if !ok {
			return failed
		}
		nr, ok := count(fields["noResultCount"])
		if !ok || nr > n || !ratio(fields["rate"], nr, n) {
			return failed
		}
		if sum, ok = add(sum, n); !ok {
			return failed
		}
		if sumEmpty, ok = add(sumEmpty, nr); !ok {
			return failed
		}
	}
	if total != sum || empty != sumEmpty {
		return failed
	}
	var rate *float64
	if total > 0 {
		value := float64(empty) / float64(total)
		rate = &value
	}
	return Analytics{Status: "connected", ReportedDays: len(dates), Searches: &total, NoResultSearches: &empty, NoResultRate: &rate}
}

// ParseAlgoliaAggregate normalizes the same bounded count/rate receipt whether
// it came from the fixed REST endpoint or a schema-verified MCP aggregate tool.
// Neither caller can forward search records or raw query text through this DTO.
func ParseAlgoliaAggregate(raw []byte, window Window) Analytics {
	if len(raw) > 2<<20 {
		return Analytics{Status: "unavailable"}
	}
	return parseAnalytics(raw, window)
}
