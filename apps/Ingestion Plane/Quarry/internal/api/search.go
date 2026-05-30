package api

import (
	"context"
	"errors"
	"net/http"
	"net/url"
	"strings"

	"github.com/gofiber/fiber/v2"
	"golang.org/x/sync/errgroup"

	"github.com/triodelab/quarry/internal/scraper"
	quarrysearch "github.com/triodelab/quarry/internal/search"
)

type searchRequest struct {
	URL               string   `json:"url"`
	Site              string   `json:"site,omitempty"`
	Query             string   `json:"query"`
	Limit             int      `json:"limit,omitempty"`
	IncludeSubdomains bool     `json:"includeSubdomains,omitempty"`
	Sources           []string `json:"sources,omitempty"` // "web", "news", "images" (global search, requires external API)
}

type searchResult struct {
	Title   string  `json:"title"`
	URL     string  `json:"url"`
	Snippet string  `json:"snippet,omitempty"`
	Source  string  `json:"source"`
	Type    string  `json:"type"`
	Score   float64 `json:"score,omitempty"`
}

func (h *Handler) searchURLs(c *fiber.Ctx) error {
	var req searchRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	if strings.TrimSpace(req.Query) == "" {
		return writeError(c, http.StatusBadRequest, "query is required", nil)
	}
	if req.Limit <= 0 {
		req.Limit = 20
	}
	if req.Limit > defaultSearchLimitMax {
		return writeError(c, http.StatusBadRequest, "limit exceeds configured maximum", nil)
	}
	req.URL = strings.TrimSpace(req.URL)
	req.Site = strings.TrimSpace(req.Site)

	sources, err := normalizeSearchSources(req.Sources)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}

	if len(sources) > 0 {
		siteFilter, siteErr := normalizeSearchSite(req.URL, req.Site)
		if siteErr != nil {
			return writeError(c, http.StatusBadRequest, siteErr.Error(), nil)
		}
		results, searchErr := h.runGlobalSearch(c.UserContext(), req.Query, req.Limit, siteFilter, sources)
		if searchErr != nil {
			return writeSearchError(c, searchErr)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"query":   req.Query,
			"count":   len(results),
			"results": results,
		})
	}

	baseURL, err := resolveSiteSearchBaseURL(req.URL, req.Site)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if baseURL == "" {
		return writeError(c, http.StatusBadRequest, "url is required for site-based search", nil)
	}
	if err := validateAbsoluteHTTPURL(baseURL); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}

	parsedBaseURL, parseErr := url.ParseRequestURI(baseURL)
	if parseErr != nil {
		return writeError(c, http.StatusBadRequest, "url must be a valid absolute URL", nil)
	}

	outputs, _, err := h.fetchFormats(c.UserContext(), baseURL, &scraper.FormatOptions{Formats: []string{"links"}})
	if err != nil {
		return writeError(c, http.StatusBadGateway, "search failed", err.Error())
	}

	rawLinks, _ := outputs["links"].([]string)
	queryLower := strings.ToLower(strings.TrimSpace(req.Query))
	matches := make([]searchResult, 0, req.Limit)
	for _, link := range rawLinks {
		parsedLink, linkErr := url.Parse(link)
		if linkErr != nil || parsedLink.Host == "" {
			continue
		}
		if !sameDomain(parsedBaseURL.Hostname(), parsedLink.Hostname(), req.IncludeSubdomains) {
			continue
		}
		if strings.Contains(strings.ToLower(link), queryLower) {
			matches = append(matches, searchResult{
				Title:  link,
				URL:    link,
				Source: "site",
				Type:   "link",
			})
			if len(matches) >= req.Limit {
				break
			}
		}
	}

	return c.JSON(fiber.Map{
		"success": true,
		"query":   req.Query,
		"count":   len(matches),
		"results": matches,
	})
}

func (h *Handler) runGlobalSearch(ctx context.Context, query string, limit int, site string, sources []quarrysearch.SearchType) ([]searchResult, error) {
	if h.searchClient == nil || !h.searchClient.Enabled() {
		return nil, errors.New("global search is not configured (BRAVE_SEARCH_API_KEY)")
	}

	resultsBySource := make([][]quarrysearch.Result, len(sources))
	group, groupCtx := errgroup.WithContext(ctx)

	for index, source := range sources {
		index := index
		source := source
		group.Go(func() error {
			results, err := h.searchClient.Search(groupCtx, source, quarrysearch.SearchOptions{
				Query: query,
				Limit: limit,
				Site:  site,
			})
			if err != nil {
				return err
			}
			resultsBySource[index] = results
			return nil
		})
	}

	if err := group.Wait(); err != nil {
		return nil, err
	}

	flattened := make([]searchResult, 0, limit*len(sources))
	for _, results := range resultsBySource {
		for _, result := range results {
			flattened = append(flattened, searchResult{
				Title:   result.Title,
				URL:     result.URL,
				Snippet: result.Snippet,
				Source:  result.Source,
				Type:    result.Type,
				Score:   result.Score,
			})
		}
	}
	return flattened, nil
}

func normalizeSearchSources(raw []string) ([]quarrysearch.SearchType, error) {
	if len(raw) == 0 {
		return nil, nil
	}

	seen := make(map[quarrysearch.SearchType]struct{}, len(raw))
	sources := make([]quarrysearch.SearchType, 0, len(raw))
	for _, item := range raw {
		switch normalized := quarrysearch.SearchType(strings.ToLower(strings.TrimSpace(item))); normalized {
		case quarrysearch.SearchTypeWeb, quarrysearch.SearchTypeNews, quarrysearch.SearchTypeImages:
			if _, exists := seen[normalized]; !exists {
				seen[normalized] = struct{}{}
				sources = append(sources, normalized)
			}
		default:
			return nil, errors.New("sources contains unsupported value")
		}
	}
	return sources, nil
}

func resolveSiteSearchBaseURL(rawURL, rawSite string) (string, error) {
	if strings.TrimSpace(rawURL) != "" {
		return rawURL, nil
	}
	if strings.TrimSpace(rawSite) == "" {
		return "", nil
	}
	if strings.HasPrefix(rawSite, "http://") || strings.HasPrefix(rawSite, "https://") {
		return rawSite, nil
	}
	return "https://" + rawSite, nil
}

func normalizeSearchSite(rawURL, rawSite string) (string, error) {
	baseURL, err := resolveSiteSearchBaseURL(rawURL, rawSite)
	if err != nil || strings.TrimSpace(baseURL) == "" {
		return "", err
	}
	if validateErr := validateAbsoluteHTTPURL(baseURL); validateErr != nil {
		return "", validateErr
	}
	parsed, parseErr := url.Parse(baseURL)
	if parseErr != nil {
		return "", parseErr
	}
	return parsed.Hostname(), nil
}

func writeSearchError(c *fiber.Ctx, err error) error {
	var apiErr *quarrysearch.APIError
	if errors.As(err, &apiErr) {
		status := http.StatusBadGateway
		if apiErr.StatusCode == http.StatusTooManyRequests {
			status = http.StatusServiceUnavailable
		}
		return writeError(c, status, "global search failed", apiErr.Message)
	}
	if strings.Contains(err.Error(), "not configured") {
		return writeError(c, http.StatusServiceUnavailable, err.Error(), nil)
	}
	return writeError(c, http.StatusBadGateway, "global search failed", err.Error())
}
