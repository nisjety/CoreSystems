package http

import (
	"context"
	"net/http"
	"strconv"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type SearchGetter interface {
	GetSearch(context.Context, string, string) pulse.SearchProjection
}

func searchHandler(source PulseGetter) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", http.MethodGet)
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"error": map[string]string{"code": "method_not_allowed", "message": "GET is required"}})
			return
		}
		query := r.URL.Query()
		if len(query) > 2 || len(query["quarter"]) > 1 || len(query["site"]) != 1 {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "one site and at most one quarter parameter are required"}})
			return
		}
		for key := range query {
			if key != "site" && key != "quarter" {
				writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "unknown query parameter"}})
				return
			}
		}
		site, err := pulse.CanonicalSite(query.Get("site"))
		if err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_site", "message": "site must be a public HTTPS website origin"}})
			return
		}
		quarter := query.Get("quarter")
		period, err := pulse.QuarterPeriod(quarter, time.Now())
		if err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_quarter", "message": "quarter must be a current or past quarter in this year"}})
			return
		}
		projection := pulse.SearchProjection{
			Site: site, Source: "google_search_console", Status: pulse.StatusUnavailable, Period: period,
			Queries: []pulse.SearchRow{}, Pages: []pulse.SearchRow{}, Opportunities: []pulse.Opportunity{}, RowLimit: pulse.SearchRowLimit, Reason: pulse.ReasonNotConfigured,
		}
		if search, ok := source.(SearchGetter); ok {
			projection = search.GetSearch(r.Context(), quarter, site)
		}
		writeJSON(w, http.StatusOK, map[string]any{"data": projection})
	}
}

type SearchComparisonGetter interface {
	GetSearchComparison(context.Context, string, string, pulse.Period, pulse.Period, int) pulse.SearchComparisonProjection
}

func searchComparisonHandler(source PulseGetter) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", http.MethodGet)
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"error": map[string]string{"code": "method_not_allowed", "message": "GET is required"}})
			return
		}
		query, err := strictQuery(r)
		if err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "query parameters are invalid"}})
			return
		}
		allowed := map[string]bool{"site": true, "currentStart": true, "currentEnd": true, "previousStart": true, "previousEnd": true, "dimension": true, "rowLimit": true}
		for key, values := range query {
			if !allowed[key] || len(values) != 1 {
				writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "unknown or repeated query parameter"}})
				return
			}
		}
		for key := range allowed {
			if key != "dimension" && key != "rowLimit" && len(query[key]) != 1 {
				writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "site and both comparison periods are required"}})
				return
			}
		}
		if (len(query["dimension"]) == 1 && query.Get("dimension") == "") || (len(query["rowLimit"]) == 1 && query.Get("rowLimit") == "") {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "optional query parameters cannot be empty"}})
			return
		}
		site, err := pulse.CanonicalSite(query.Get("site"))
		if err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_site", "message": "site must be a public HTTPS website origin"}})
			return
		}
		current, previous, err := pulse.ComparisonPeriods(query.Get("currentStart"), query.Get("currentEnd"), query.Get("previousStart"), query.Get("previousEnd"), time.Now())
		if err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_period", "message": "periods must be adjacent, equal length (up to 90 days), completed dates"}})
			return
		}
		dimension := query.Get("dimension")
		if dimension == "" {
			dimension = "query"
		}
		if dimension != "query" && dimension != "page" {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_dimension", "message": "dimension must be query or page"}})
			return
		}
		rowLimit := pulse.SearchComparisonRowLimitDefault
		if raw := query.Get("rowLimit"); raw != "" {
			rowLimit, err = strconv.Atoi(raw)
			if err != nil || rowLimit < 1 || rowLimit > pulse.SearchComparisonRowLimitMax {
				writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_row_limit", "message": "rowLimit must be between 1 and 1000"}})
				return
			}
		}
		projection := pulse.SearchComparisonProjection{Site: site, Source: "google_search_console", Status: pulse.StatusUnavailable, Dimension: dimension, CurrentPeriod: current, PreviousPeriod: previous, Rows: []pulse.SearchComparisonRow{}, RowLimit: rowLimit, Reason: pulse.ReasonNotConfigured}
		if comparison, ok := source.(SearchComparisonGetter); ok {
			projection = comparison.GetSearchComparison(r.Context(), site, dimension, current, previous, rowLimit)
		}
		writeJSON(w, http.StatusOK, map[string]any{"data": projection})
	}
}
