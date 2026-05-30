package api

import (
	"context"
	"encoding/xml"
	"io"
	"net/http"
	"net/url"
	"sort"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/search"
)

type mapRequest struct {
	URL               string `json:"url"`
	Search            string `json:"search,omitempty"`
	Limit             int    `json:"limit,omitempty"`
	IncludeSubdomains bool   `json:"includeSubdomains,omitempty"`
	IgnoreSitemap     bool   `json:"ignoreSitemap,omitempty"`
}

func (h *Handler) mapURLs(c *fiber.Ctx) error {
	if h.scraper == nil {
		return writeError(c, http.StatusServiceUnavailable, "scraper is not initialized", nil)
	}

	var req mapRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	if strings.TrimSpace(req.URL) == "" {
		return writeError(c, http.StatusBadRequest, "url is required", nil)
	}
	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	parsed, err := url.ParseRequestURI(req.URL)
	if err != nil {
		return writeError(c, http.StatusBadRequest, "url must be a valid absolute URL", nil)
	}
	if req.Limit <= 0 {
		req.Limit = 100
	}
	if req.Limit > defaultMapLimitMax {
		return writeError(c, http.StatusBadRequest, "limit exceeds configured maximum", nil)
	}

	outputs, _, fetchErr := h.scraper.FetchFormats(c.UserContext(), req.URL, &scraper.FormatOptions{Formats: []string{"links"}})
	if fetchErr != nil {
		return writeError(c, http.StatusBadGateway, "failed to map urls", fetchErr.Error())
	}

	rawLinks, _ := outputs["links"].([]string)

	// Merge sitemap URLs unless ignored
	if !req.IgnoreSitemap {
		sitemapURLs := fetchSitemapURLs(c.UserContext(), parsed)
		rawLinks = append(rawLinks, sitemapURLs...)
	}

	seen := make(map[string]struct{}, len(rawLinks))
	filtered := make([]string, 0, len(rawLinks))
	for _, link := range rawLinks {
		link = strings.TrimSpace(link)
		if link == "" {
			continue
		}
		u, parseErr := url.Parse(link)
		if parseErr != nil || u.Host == "" {
			continue
		}
		if !sameDomain(parsed.Hostname(), u.Hostname(), req.IncludeSubdomains) {
			continue
		}
		if _, ok := seen[link]; ok {
			continue
		}
		seen[link] = struct{}{}
		filtered = append(filtered, link)
		if len(filtered) >= req.Limit {
			break
		}
	}

	sort.Strings(filtered)

	// Semantic re-ranking: when a search query is supplied, reorder filtered
	// URLs by their relevance to the query. Falls back to TF-IDF when the AI
	// backend is unavailable so the endpoint always returns promptly.
	if req.Search != "" && len(filtered) > 1 {
		var embClient search.EmbeddingClient
		if h.scraper != nil && h.scraper.AIClient() != nil {
			embClient = h.scraper.AIClient()
		}
		rankCtx, rankCancel := context.WithTimeout(c.UserContext(), 5*time.Second)
		defer rankCancel()
		ranked, rankErr := search.RankByEmbedding(rankCtx, embClient, req.Search, filtered)
		if rankErr == nil {
			filtered = ranked
		}
	}

	return c.JSON(fiber.Map{
		"success": true,
		"url":     req.URL,
		"count":   len(filtered),
		"links":   filtered,
	})
}

// --- Sitemap parsing ---

type sitemapURLSet struct {
	XMLName xml.Name     `xml:"urlset"`
	URLs    []sitemapURL `xml:"url"`
}

type sitemapURL struct {
	Loc string `xml:"loc"`
}

type sitemapIndex struct {
	XMLName  xml.Name       `xml:"sitemapindex"`
	Sitemaps []sitemapEntry `xml:"sitemap"`
}

type sitemapEntry struct {
	Loc string `xml:"loc"`
}

// fetchSitemapURLs fetches /sitemap.xml and parses <loc> entries.
// If it encounters a sitemapindex, it follows child sitemaps (max 5, 1 level deep).
func fetchSitemapURLs(ctx context.Context, base *url.URL) []string {
	sitemapURL := base.Scheme + "://" + base.Host + "/sitemap.xml"

	data, err := fetchURL(ctx, sitemapURL)
	if err != nil {
		log.Debug().Err(err).Str("url", sitemapURL).Msg("sitemap fetch failed")
		return nil
	}

	// Try as urlset first
	var urlSet sitemapURLSet
	if err := xml.Unmarshal(data, &urlSet); err == nil && len(urlSet.URLs) > 0 {
		urls := make([]string, 0, len(urlSet.URLs))
		for _, u := range urlSet.URLs {
			if loc := strings.TrimSpace(u.Loc); loc != "" {
				urls = append(urls, loc)
			}
		}
		log.Info().Int("count", len(urls)).Msg("parsed sitemap.xml urlset")
		return urls
	}

	// Try as sitemapindex
	var idx sitemapIndex
	if err := xml.Unmarshal(data, &idx); err == nil && len(idx.Sitemaps) > 0 {
		var urls []string
		limit := len(idx.Sitemaps)
		if limit > 5 {
			limit = 5
		}
		for _, entry := range idx.Sitemaps[:limit] {
			childData, err := fetchURL(ctx, entry.Loc)
			if err != nil {
				continue
			}
			var childSet sitemapURLSet
			if err := xml.Unmarshal(childData, &childSet); err == nil {
				for _, u := range childSet.URLs {
					if loc := strings.TrimSpace(u.Loc); loc != "" {
						urls = append(urls, loc)
					}
				}
			}
		}
		log.Info().Int("count", len(urls)).Int("child_sitemaps", limit).Msg("parsed sitemapindex")
		return urls
	}

	return nil
}

// fetchURL is a simple HTTP GET with timeout, returns body bytes.
func fetchURL(ctx context.Context, rawURL string) ([]byte, error) {
	reqCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()

	httpReq, err := http.NewRequestWithContext(reqCtx, http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, err
	}
	httpReq.Header.Set("User-Agent", "Quarry/1.0 Sitemap-Fetcher")

	resp, err := http.DefaultClient.Do(httpReq)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, &url.Error{Op: "GET", URL: rawURL, Err: io.EOF}
	}

	// Limit read to 5MB
	return io.ReadAll(io.LimitReader(resp.Body, 5<<20))
}

func sameDomain(baseHost, candidateHost string, includeSubdomains bool) bool {
	baseHost = strings.ToLower(strings.TrimSpace(baseHost))
	candidateHost = strings.ToLower(strings.TrimSpace(candidateHost))
	if baseHost == "" || candidateHost == "" {
		return false
	}
	if candidateHost == baseHost {
		return true
	}
	if includeSubdomains {
		return strings.HasSuffix(candidateHost, "."+baseHost)
	}
	return false
}
