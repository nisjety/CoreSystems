package pulse

import (
	"context"
	"strings"
	"time"
)

const SearchRowLimit = 100

type SearchRow struct {
	Key         string  `json:"key"`
	Clicks      int64   `json:"clicks"`
	Impressions int64   `json:"impressions"`
	CTR         float64 `json:"ctr"`
	Position    float64 `json:"position"`
}

// Opportunity is a transparent review heuristic, not a task, forecast, or an
// instruction to change content automatically. Its metrics are observed rows.
type Opportunity struct {
	SearchRow
	Type string `json:"type"`
}

type SearchProjection struct {
	Site              string        `json:"site"`
	Source            string        `json:"source"`
	Status            string        `json:"status"`
	Period            Period        `json:"period"`
	Queries           []SearchRow   `json:"queries"`
	Pages             []SearchRow   `json:"pages"`
	Opportunities     []Opportunity `json:"opportunities"`
	RowLimit          int           `json:"rowLimit"`
	QueriesAtLimit    bool          `json:"queriesAtLimit"`
	PagesAtLimit      bool          `json:"pagesAtLimit"`
	PropertyScope     string        `json:"propertyScope,omitempty"`
	ReportingProperty string        `json:"reportingProperty,omitempty"`
	UpdatedAt         string        `json:"updatedAt,omitempty"`
	Reason            string        `json:"reason,omitempty"`
}

func unavailableSearch(site string, period Period, reason string) SearchProjection {
	return SearchProjection{
		Site: site, Source: "google_search_console", Status: StatusUnavailable,
		Period: period, Reason: reason, RowLimit: SearchRowLimit,
		Queries: []SearchRow{}, Pages: []SearchRow{}, Opportunities: []Opportunity{},
	}
}

// GetSearch reads only fixed Search Console operations through Integration Core.
// A verified property is required before either dimension is requested; one
// failed read invalidates the whole projection instead of showing partial data.
func (s *Service) GetSearch(ctx context.Context, quarter, requestedSite string) SearchProjection {
	now := s.now().UTC()
	site, siteErr := CanonicalSite(requestedSite)
	period, err := QuarterPeriod(quarter, now)
	if err != nil {
		return unavailableSearch(site, Period{}, ReasonSourceUnavailable)
	}
	if siteErr != nil {
		return unavailableSearch(site, period, ReasonPropertyUnverified)
	}
	if !s.cfg.complete() || validateConfig(s.cfg) != nil {
		return unavailableSearch(site, period, ReasonNotConfigured)
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	token, err := readBearerToken(s.cfg.IntegrationBearerTokenFile)
	if err != nil {
		return unavailableSearch(site, period, ReasonSourceUnavailable)
	}
	property, err := s.verifiedProperty(ctx, token, site)
	if err != nil {
		return unavailableSearch(site, period, ReasonSourceUnavailable)
	}
	if property == "" {
		return unavailableSearch(site, period, ReasonPropertyUnverified)
	}
	queries, pages, err := s.readSearchDimensions(ctx, token, period, property)
	if err != nil {
		return unavailableSearch(site, period, ReasonSourceUnavailable)
	}
	return connectedSearch(site, period, property, queries, pages, now)
}

func connectedSearch(site string, period Period, property string, queries, pages []SearchRow, now time.Time) SearchProjection {
	scope := "url_prefix"
	if strings.HasPrefix(property, "sc-domain:") {
		scope = "domain"
	}
	return SearchProjection{
		Site: site, Source: "google_search_console", Status: StatusConnected, Period: period,
		Queries: queries, Pages: pages, Opportunities: searchOpportunities(queries),
		RowLimit: SearchRowLimit, QueriesAtLimit: len(queries) == SearchRowLimit, PagesAtLimit: len(pages) == SearchRowLimit,
		PropertyScope: scope, ReportingProperty: property, UpdatedAt: now.Format(time.RFC3339),
	}
}

func (s *Service) readSearchDimensions(ctx context.Context, token string, period Period, property string) ([]SearchRow, []SearchRow, error) {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	type dimensionResult struct {
		dimension string
		rows      []SearchRow
		err       error
	}
	results := make(chan dimensionResult, 2)
	for _, dimension := range []string{"query", "page"} {
		go func(dimension string) {
			rows, err := s.readSearchRows(ctx, token, period, property, dimension)
			results <- dimensionResult{dimension: dimension, rows: rows, err: err}
		}(dimension)
	}
	var queries, pages []SearchRow
	for i := 0; i < 2; i++ {
		select {
		case <-ctx.Done():
			return nil, nil, ctx.Err()
		case result := <-results:
			if result.err != nil {
				return nil, nil, result.err
			}
			if result.dimension == "query" {
				queries = result.rows
			} else {
				pages = result.rows
			}
		}
	}
	return queries, pages, nil
}
