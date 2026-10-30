package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type fakeSearch struct {
	fakePulse
	searchQuarter string
	searchSite    string
	comparison    pulse.SearchComparisonProjection
	compareCalls  int
}

func (f *fakeSearch) GetSearch(_ context.Context, quarter, site string) pulse.SearchProjection {
	f.searchQuarter, f.searchSite = quarter, site
	return pulse.SearchProjection{Site: site, Source: "google_search_console", Status: pulse.StatusConnected, Queries: []pulse.SearchRow{}, Pages: []pulse.SearchRow{}, Opportunities: []pulse.Opportunity{}, RowLimit: 100}
}

func (f *fakeSearch) GetSearchComparison(_ context.Context, site, dimension string, current, previous pulse.Period, rowLimit int) pulse.SearchComparisonProjection {
	f.compareCalls++
	f.searchSite = site
	f.comparison = pulse.SearchComparisonProjection{Site: site, Status: pulse.StatusConnected, Dimension: dimension, CurrentPeriod: current, PreviousPeriod: previous, RowLimit: rowLimit, Rows: []pulse.SearchComparisonRow{}}
	return f.comparison
}

func TestSearchRouteUsesProtectedReadOnlyProjection(t *testing.T) {
	source := &fakeSearch{}
	handler := NewHandler("internal-test-key", source)
	path := "/api/v1/marketing/search?quarter=Q1&site=https%3A%2F%2Faquatiq.com%2F"
	unauthorized := httptest.NewRecorder()
	handler.ServeHTTP(unauthorized, httptest.NewRequest(http.MethodGet, path, nil))
	if unauthorized.Code != http.StatusUnauthorized || source.searchSite != "" {
		t.Fatalf("unauthorized=%d", unauthorized.Code)
	}
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.Header.Set("X-Internal-API-Key", "internal-test-key")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, req)
	if response.Code != http.StatusOK || source.searchQuarter != "Q1" || source.searchSite != "https://aquatiq.com/" || response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("response=%d %s source=%#v", response.Code, response.Body.String(), source)
	}
	var body struct {
		Data pulse.SearchProjection `json:"data"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || body.Data.RowLimit != 100 {
		t.Fatalf("body=%s err=%v", response.Body.String(), err)
	}
}

func TestSearchRouteRejectsInvalidInputBeforeReadingProvider(t *testing.T) {
	source := &fakeSearch{}
	handler := NewHandler("internal-test-key", source)
	for _, tc := range []struct {
		method, path string
		status       int
	}{
		{http.MethodPost, "/api/v1/marketing/search", 405},
		{http.MethodGet, "/api/v1/marketing/search?site=https://aquatiq.com/&quarter=Q5", 400},
		{http.MethodGet, "/api/v1/marketing/search?quarter=Q1", 400},
		{http.MethodGet, "/api/v1/marketing/search?site=https://localhost/", 400},
		{http.MethodGet, "/api/v1/marketing/search?site=https://aquatiq.com/path", 400},
		{http.MethodGet, "/api/v1/marketing/search?site=https://aquatiq.com/&extra=1", 400},
		{http.MethodGet, "/api/v1/marketing/search?site=https://aquatiq.com/&site=https://other.com/", 400},
		{http.MethodGet, "/api/v1/marketing/search?site=https://aquatiq.com/&quarter=Q1&quarter=Q2", 400},
	} {
		req := httptest.NewRequest(tc.method, tc.path, nil)
		req.Header.Set("X-Internal-API-Key", "internal-test-key")
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != tc.status || source.searchSite != "" || response.Header().Get("Cache-Control") != "no-store" {
			t.Fatalf("%s %s=%d %s", tc.method, tc.path, response.Code, response.Body.String())
		}
	}
}

func TestSearchRouteHasSafeFallbackForPulseOnlySources(t *testing.T) {
	for _, handler := range []http.Handler{NewHandler("test"), NewHandler("test", &fakePulse{})} {
		req := httptest.NewRequest(http.MethodGet, "/api/v1/marketing/search?quarter=Q1&site=https://aquatiq.com/", nil)
		req.Header.Set("X-Internal-API-Key", "test")
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		var body struct {
			Data pulse.SearchProjection `json:"data"`
		}
		if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || response.Code != 200 || body.Data.Status != pulse.StatusUnavailable || body.Data.Reason != pulse.ReasonNotConfigured || body.Data.Queries == nil || body.Data.Pages == nil || body.Data.Opportunities == nil {
			t.Fatalf("response=%d %s err=%v", response.Code, response.Body.String(), err)
		}
	}
}

func TestSearchCompareRouteRequiresKeyAndValidatesWindows(t *testing.T) {
	source := &fakeSearch{}
	handler := NewHandler("internal-test-key", source)
	today := time.Now().UTC().Truncate(24 * time.Hour)
	currentEnd := today.AddDate(0, 0, -1)
	currentStart := currentEnd.AddDate(0, 0, -27)
	previousEnd := currentStart.AddDate(0, 0, -1)
	previousStart := previousEnd.AddDate(0, 0, -27)
	query := "?site=https%3A%2F%2Fexample.com%2F&dimension=query&currentStart=" + currentStart.Format("2006-01-02") + "&currentEnd=" + currentEnd.Format("2006-01-02") + "&previousStart=" + previousStart.Format("2006-01-02") + "&previousEnd=" + previousEnd.Format("2006-01-02")
	path := "/api/v1/marketing/search/compare" + query
	unauthorized := httptest.NewRecorder()
	handler.ServeHTTP(unauthorized, httptest.NewRequest(http.MethodGet, path, nil))
	if unauthorized.Code != http.StatusUnauthorized || source.compareCalls != 0 {
		t.Fatalf("unauthorized=%d calls=%d", unauthorized.Code, source.compareCalls)
	}
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.Header.Set("X-Internal-API-Key", "internal-test-key")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, req)
	if response.Code != http.StatusOK || source.compareCalls != 1 || source.comparison.RowLimit != pulse.SearchComparisonRowLimitDefault || source.comparison.Dimension != "query" || response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("response=%d %s projection=%#v calls=%d", response.Code, response.Body.String(), source.comparison, source.compareCalls)
	}
	for _, invalid := range []string{"&dimension=other", "&dimension=", "&rowLimit=5001", "&rowLimit=", "&extra=1"} {
		badReq := httptest.NewRequest(http.MethodGet, "/api/v1/marketing/search/compare"+query+invalid, nil)
		badReq.Header.Set("X-Internal-API-Key", "internal-test-key")
		bad := httptest.NewRecorder()
		handler.ServeHTTP(bad, badReq)
		if bad.Code != http.StatusBadRequest || source.compareCalls != 1 {
			t.Fatalf("invalid query %q = %d %s calls=%d", invalid, bad.Code, bad.Body.String(), source.compareCalls)
		}
	}
}
