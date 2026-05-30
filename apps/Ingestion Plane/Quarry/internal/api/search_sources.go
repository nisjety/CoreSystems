package api

import (
	"context"
	"fmt"
	neturl "net/url"
	"sort"
	"strings"

	"github.com/triodelab/quarry/internal/dataplane"
	quarrysearch "github.com/triodelab/quarry/internal/search"
)

const (
	searchSourceDocuments = "documents"
	searchSourceGitHub    = "github"
	searchSourceIndex     = "index"
	searchSourceLocal     = "local"
	searchBlendInterleave = "interleave"
	searchBlendRanked     = "ranked"
)

type searchCandidate struct {
	result      quarrysearch.Result
	score       float64
	sourceIndex int
	position    int
}

func (h *Handler) ensureSearchSourcesAvailable(sources []v2SearchSource) error {
	if len(sources) == 0 {
		sources = []v2SearchSource{{Type: string(quarrysearch.SearchTypeWeb)}}
	}
	for _, source := range sources {
		switch strings.ToLower(strings.TrimSpace(source.Type)) {
		case string(quarrysearch.SearchTypeWeb), string(quarrysearch.SearchTypeNews), string(quarrysearch.SearchTypeImages):
			if h.searchClient == nil || !h.searchClient.Enabled() {
				return fmt.Errorf("search is not configured (set BRAVE_SEARCH_API_KEY)")
			}
		case searchSourceGitHub:
			if h.githubSearchClient == nil {
				return fmt.Errorf("github search is not configured")
			}
		case searchSourceLocal:
			if h.localSearchIndex == nil || !h.localSearchIndex.Enabled() {
				return fmt.Errorf("local search index is not enabled (set SEARCH_INDEX_ENABLED=true)")
			}
		case searchSourceDocuments, searchSourceIndex:
			if h.dataplaneClient == nil || !h.dataplaneClient.RetrievalConfigured() {
				return fmt.Errorf("dataplane retrieval is not configured")
			}
		default:
			return fmt.Errorf("sources contains unsupported value")
		}
	}
	return nil
}

func (h *Handler) executeSearchSource(ctx context.Context, orgID string, cfg v2SearchRunConfig, source v2SearchSource) ([]quarrysearch.Result, error) {
	limit := searchLimitForSource(source, cfg.Limit)
	var (
		results []quarrysearch.Result
		err     error
	)
	switch strings.ToLower(strings.TrimSpace(source.Type)) {
	case string(quarrysearch.SearchTypeWeb), string(quarrysearch.SearchTypeNews), string(quarrysearch.SearchTypeImages):
		results, err = h.searchClient.Search(ctx, quarrysearch.SearchType(source.Type), quarrysearch.SearchOptions{
			Query:         cfg.Query,
			Limit:         limit,
			Site:          source.Site,
			Country:       source.Country,
			SearchLang:    source.SearchLang,
			UILang:        source.UILang,
			Freshness:     source.Freshness,
			SafeSearch:    source.SafeSearch,
			ExtraSnippets: source.ExtraSnippets,
		})
	case searchSourceGitHub:
		results, err = h.githubSearchClient.SearchRepositories(ctx, quarrysearch.SearchOptions{
			Query: cfg.Query,
			Limit: limit,
			Site:  source.Site,
		})
	case searchSourceLocal:
		results, err = h.localSearchIndex.Query(ctx, cfg.Query, limit)
	case searchSourceIndex:
		results, err = h.retrieveSearchResults(ctx, orgID, cfg.Query, limit, searchSourceIndex)
	case searchSourceDocuments:
		results, err = h.retrieveSearchResults(ctx, orgID, cfg.Query, limit, searchSourceDocuments)
	default:
		return nil, fmt.Errorf("sources contains unsupported value")
	}
	if err != nil {
		return nil, err
	}
	if limit > 0 && len(results) > limit {
		results = append([]quarrysearch.Result(nil), results[:limit]...)
	}
	return results, nil
}

func (h *Handler) retrieveSearchResults(ctx context.Context, orgID, query string, limit int, sourceType string) ([]quarrysearch.Result, error) {
	if h.dataplaneClient == nil {
		return nil, fmt.Errorf("dataplane retrieval is not configured")
	}
	response, err := h.dataplaneClient.Retrieve(ctx, orgID, &dataplane.RetrieveRequest{
		Query: query,
		TopK:  min(max(limit*3, limit), 100),
		TopN:  limit,
	})
	if err != nil {
		return nil, err
	}
	switch sourceType {
	case searchSourceIndex:
		return mapRetrieveFactsToSearchResults(response.Facts, response.Sources), nil
	case searchSourceDocuments:
		return mapRetrieveDocumentsToSearchResults(response.Facts, response.Sources), nil
	default:
		return nil, fmt.Errorf("sources contains unsupported value")
	}
}

func mapRetrieveFactsToSearchResults(facts []dataplane.RetrieveFact, sources []dataplane.RetrieveSource) []quarrysearch.Result {
	sourceByDocumentID := make(map[string]dataplane.RetrieveSource, len(sources))
	for _, source := range sources {
		sourceByDocumentID[strings.TrimSpace(source.DocumentID)] = source
	}

	results := make([]quarrysearch.Result, 0, len(facts))
	for _, fact := range facts {
		knowledgeID := strings.TrimSpace(fact.KnowledgeID)
		if knowledgeID == "" {
			continue
		}
		title := strings.TrimSpace(stringFromMap(fact.Metadata, "title"))
		if title == "" {
			if source, ok := sourceByDocumentID[strings.TrimSpace(fact.DocumentID)]; ok {
				title = strings.TrimSpace(source.Title)
			}
		}
		if title == "" {
			title = "Indexed knowledge " + knowledgeID
		}
		results = append(results, quarrysearch.Result{
			Title:   title,
			URL:     "dataplane://knowledge/" + knowledgeID,
			Snippet: strings.TrimSpace(fact.Text),
			Source:  "dataplane",
			Type:    searchSourceIndex,
			Score:   retrievalScore(fact),
		})
	}
	return results
}

func mapRetrieveDocumentsToSearchResults(facts []dataplane.RetrieveFact, sources []dataplane.RetrieveSource) []quarrysearch.Result {
	snippetsByDocumentID := make(map[string]string, len(facts))
	titlesByDocumentID := make(map[string]string, len(facts))
	for _, fact := range facts {
		documentID := strings.TrimSpace(fact.DocumentID)
		if documentID == "" {
			continue
		}
		if snippetsByDocumentID[documentID] == "" {
			snippetsByDocumentID[documentID] = strings.TrimSpace(fact.Text)
		}
		if titlesByDocumentID[documentID] == "" {
			titlesByDocumentID[documentID] = strings.TrimSpace(stringFromMap(fact.Metadata, "title"))
		}
	}

	results := make([]quarrysearch.Result, 0, len(sources))
	seen := make(map[string]struct{}, len(sources))
	for _, source := range sources {
		documentID := strings.TrimSpace(source.DocumentID)
		if documentID == "" {
			continue
		}
		if _, exists := seen[documentID]; exists {
			continue
		}
		seen[documentID] = struct{}{}

		title := strings.TrimSpace(source.Title)
		if title == "" {
			title = titlesByDocumentID[documentID]
		}
		if title == "" {
			title = "Document " + documentID
		}
		results = append(results, quarrysearch.Result{
			Title:   title,
			URL:     "dataplane://documents/" + documentID,
			Snippet: snippetsByDocumentID[documentID],
			Source:  "dataplane",
			Type:    searchSourceDocuments,
			Score:   1,
		})
	}
	if len(results) > 0 {
		return results
	}
	for documentID, snippet := range snippetsByDocumentID {
		title := titlesByDocumentID[documentID]
		if title == "" {
			title = "Document " + documentID
		}
		results = append(results, quarrysearch.Result{
			Title:   title,
			URL:     "dataplane://documents/" + documentID,
			Snippet: snippet,
			Source:  "dataplane",
			Type:    searchSourceDocuments,
			Score:   1,
		})
	}
	return results
}

func isScrapableSearchURL(raw string) bool {
	parsed, err := neturl.Parse(strings.TrimSpace(raw))
	if err != nil {
		return false
	}
	return parsed.Scheme == "http" || parsed.Scheme == "https"
}

func stringFromMap(values map[string]interface{}, key string) string {
	if len(values) == 0 {
		return ""
	}
	value, _ := values[key]
	switch typed := value.(type) {
	case string:
		return typed
	default:
		return ""
	}
}

func normalizeBlendMode(raw string) string {
	mode := strings.ToLower(strings.TrimSpace(raw))
	switch mode {
	case "", searchBlendRanked:
		return searchBlendRanked
	case searchBlendInterleave:
		return searchBlendInterleave
	default:
		return ""
	}
}

func searchLimitForSource(source v2SearchSource, global int) int {
	if source.Limit > 0 {
		if global > 0 && source.Limit > global {
			return global
		}
		return source.Limit
	}
	return global
}

func searchWeightForSource(source v2SearchSource) float64 {
	if source.Weight > 0 {
		return source.Weight
	}
	return 1
}

func retrievalScore(fact dataplane.RetrieveFact) float64 {
	if fact.RerankScore != nil && *fact.RerankScore > 0 {
		return *fact.RerankScore
	}
	if fact.Score > 0 {
		return fact.Score
	}
	return 1
}

func blendSearchResults(resultsBySource [][]quarrysearch.Result, sources []v2SearchSource, blendMode string, limit int) []quarrysearch.Result {
	if len(resultsBySource) == 0 {
		return nil
	}
	if normalizeBlendMode(blendMode) == searchBlendInterleave {
		return interleaveSearchResults(resultsBySource, sources, limit)
	}
	return rankSearchResults(resultsBySource, sources, limit)
}

func rankSearchResults(resultsBySource [][]quarrysearch.Result, sources []v2SearchSource, limit int) []quarrysearch.Result {
	candidates := flattenSearchCandidates(resultsBySource, sources)
	sort.SliceStable(candidates, func(i, j int) bool {
		if candidates[i].score == candidates[j].score {
			if candidates[i].sourceIndex == candidates[j].sourceIndex {
				return candidates[i].position < candidates[j].position
			}
			return candidates[i].sourceIndex < candidates[j].sourceIndex
		}
		return candidates[i].score > candidates[j].score
	})
	return dedupeSearchCandidates(candidates, limit)
}

func interleaveSearchResults(resultsBySource [][]quarrysearch.Result, sources []v2SearchSource, limit int) []quarrysearch.Result {
	indexes := make([]int, len(resultsBySource))
	candidates := make([]searchCandidate, 0)
	for {
		progressed := false
		for sourceIndex, results := range resultsBySource {
			if indexes[sourceIndex] >= len(results) {
				continue
			}
			position := indexes[sourceIndex]
			indexes[sourceIndex]++
			progressed = true
			candidates = append(candidates, newSearchCandidate(results[position], sourceIndex, position, sources[sourceIndex]))
		}
		if !progressed {
			break
		}
	}
	return dedupeSearchCandidates(candidates, limit)
}

func flattenSearchCandidates(resultsBySource [][]quarrysearch.Result, sources []v2SearchSource) []searchCandidate {
	candidates := make([]searchCandidate, 0)
	for sourceIndex, results := range resultsBySource {
		for position, result := range results {
			candidates = append(candidates, newSearchCandidate(result, sourceIndex, position, sources[sourceIndex]))
		}
	}
	return candidates
}

func newSearchCandidate(result quarrysearch.Result, sourceIndex, position int, source v2SearchSource) searchCandidate {
	score := result.Score
	if score <= 0 {
		score = 1 / float64(position+1)
	}
	score *= searchWeightForSource(source)
	result.Score = score
	return searchCandidate{
		result:      result,
		score:       score,
		sourceIndex: sourceIndex,
		position:    position,
	}
}

func dedupeSearchCandidates(candidates []searchCandidate, limit int) []quarrysearch.Result {
	results := make([]quarrysearch.Result, 0, len(candidates))
	seen := make(map[string]int, len(candidates))
	for _, candidate := range candidates {
		key := canonicalSearchResultKey(candidate.result)
		if existingIndex, exists := seen[key]; exists {
			results[existingIndex] = preferSearchResult(results[existingIndex], candidate.result)
			continue
		}
		results = append(results, candidate.result)
		seen[key] = len(results) - 1
		if limit > 0 && len(results) >= limit {
			break
		}
	}
	return results
}

func canonicalSearchResultKey(result quarrysearch.Result) string {
	rawURL := strings.TrimSpace(result.URL)
	if rawURL != "" {
		parsed, err := neturl.Parse(rawURL)
		if err == nil {
			parsed.Fragment = ""
			parsed.Host = strings.ToLower(parsed.Host)
			parsed.Scheme = strings.ToLower(parsed.Scheme)
			if parsed.Path != "/" {
				parsed.Path = strings.TrimRight(parsed.Path, "/")
			}
			return parsed.String()
		}
		return rawURL
	}
	return strings.ToLower(strings.TrimSpace(result.Title)) + "|" + strings.ToLower(strings.TrimSpace(result.Snippet))
}

func preferSearchResult(existing, candidate quarrysearch.Result) quarrysearch.Result {
	if candidate.Score > existing.Score {
		existing, candidate = candidate, existing
	}
	if strings.TrimSpace(existing.Snippet) == "" && strings.TrimSpace(candidate.Snippet) != "" {
		existing.Snippet = candidate.Snippet
	}
	return existing
}
