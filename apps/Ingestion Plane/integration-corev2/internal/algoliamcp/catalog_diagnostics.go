package algoliamcp

import (
	"regexp"
	"sort"
)

var diagnosticOperation = regexp.MustCompile(`^[a-z]+(?:_[a-z]+){0,8}$`)

// ToolCatalogShape records the documented vocabulary plus at most three tightly
// filtered operational identifiers. It is diagnostics, not a call allowlist.
type ToolCatalogShape struct {
	Total              int      `json:"tool_count"`
	Unknown            int      `json:"unknown_count"`
	Documented         []string `json:"documented_present"`
	ListIndicesPresent bool     `json:"list_indices_present"`
	NoResultsPresent   bool     `json:"no_results_present"`
	SearchCountPresent bool     `json:"search_count_present"`
	UnknownOperations  []string `json:"unknown_operation_names,omitempty"`
}

func documentedCatalog(names []string) ToolCatalogShape {
	// https://www.algolia.com/doc/guides/model-context-protocol/productivity-mcp
	documented := map[string]bool{
		"algolia_search_list_indices": true, "algolia_search_index": true, "algolia_search_for_facet_values": true, "algolia_recommendations": true,
		"algolia_analytics_click_positions": true, "algolia_analytics_no_click_rate": true, "algolia_analytics_top_searches_without_clicks": true,
		"algolia_analytics_top_filters": true, "algolia_analytics_top_filters_no_results": true, "algolia_analytics_no_results_rate": true,
		"algolia_analytics_number_of_searches": true, "algolia_analytics_searches_no_results": true, "algolia_analytics_top_searches": true,
		"algolia_analytics_top_search_results": true, "algolia_analytics_number_of_users": true, "algolia_analytics_top_countries": true, "search": true, "fetch": true,
	}
	shape := ToolCatalogShape{Total: len(names), Documented: []string{}}
	seen := map[string]bool{}
	unknown := map[string]bool{}
	for _, name := range names {
		if !documented[name] {
			shape.Unknown++
			// Narrowly authorized operational metadata only: exact lowercase
			// snake-case excludes account IDs, JWTs, URLs and control characters.
			if len(name) <= 96 && diagnosticOperation.MatchString(name) {
				unknown[name] = true
			}
			continue
		}
		seen[name] = true
	}
	// Append only keys from this literal vocabulary, never provider strings.
	for name := range documented {
		if seen[name] {
			shape.Documented = append(shape.Documented, name)
		}
	}
	sort.Strings(shape.Documented)
	candidates := make([]string, 0, len(unknown))
	for name := range unknown {
		candidates = append(candidates, name)
	}
	sort.Strings(candidates)
	if len(candidates) > 3 {
		candidates = candidates[:3]
	}
	shape.UnknownOperations = candidates
	shape.ListIndicesPresent = seen[listIndices]
	shape.NoResultsPresent = seen[noResults]
	shape.SearchCountPresent = seen[searchCount]
	return shape
}
