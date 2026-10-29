package http

import (
	"context"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type analyticsSource struct {
	registrySource
	calls int
}

func (s *analyticsSource) GetGA4(_ context.Context, quarter, site string) pulse.GA4Projection {
	s.calls++
	return pulse.UnavailableGA4(site, pulse.Period{StartDate: "2026-01-01", EndDate: "2026-03-31"}, "requires_consent")
}
func (s *analyticsSource) GetClarity(_ context.Context, site string) pulse.ClarityProjection {
	s.calls++
	return pulse.UnavailableClarity(site, "not_configured")
}
func TestAnalyticsRoutesRequireBoundWebsiteAndStrictReadOnlyInputs(t *testing.T) {
	registry := newAnalyticsRegistry(t)
	website, _, err := registry.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	source := &analyticsSource{registrySource: registrySource{registry}}
	handler := NewHandler("analytics-key", source)
	query := "?websiteId=" + website.ID + "&site=https%3A%2F%2Fexample.com%2F"
	request := func(method, path, body, key string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, strings.NewReader(body))
		r.Header.Set("X-Internal-API-Key", key)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	for _, path := range []string{"/api/v1/marketing/ga4", "/api/v1/marketing/clarity"} {
		if w := request("GET", path+query, "", ""); w.Code != 401 {
			t.Fatal("unauthenticated read accepted")
		}
		if w := request("POST", path+query, "{}", "analytics-key"); w.Code != 405 {
			t.Fatalf("mutation accepted: %d", w.Code)
		}
		for _, bad := range []string{"?site=https%3A%2F%2Fexample.com%2F", query + "&organizationId=spoof", query + "&site=https%3A%2F%2Fother.example%2F", query + "&websiteId=" + website.ID, query + "&token=secret", query + "&propertyId=123"} {
			if w := request("GET", path+bad, "", "analytics-key"); w.Code != 400 {
				t.Fatalf("unsafe query accepted: %s %d", bad, w.Code)
			}
		}
		stale := strings.Replace(query, "example.com", "other.example", 1)
		if w := request("GET", path+stale, "", "analytics-key"); w.Code != 409 {
			t.Fatalf("stalewebsite accepted: %d", w.Code)
		}
		if w := request("GET", path+query, "body", "analytics-key"); w.Code != 400 {
			t.Fatal("GETbodyaccepted")
		}
		valid := query
		if strings.HasSuffix(path, "ga4") {
			valid += "&quarter=Q1"
		}
		if w := request("GET", path+valid, "", "analytics-key"); w.Code != 200 || w.Header().Get("Cache-Control") != "no-store" || strings.Contains(w.Body.String(), "analytics-key") {
			t.Fatalf("readfailed: %d %s", w.Code, w.Body.String())
		}
	}
	if source.calls != 2 {
		t.Fatal("invalid requests reached reportsource")
	}
	if w := request("GET", "/api/v1/marketing/clarity"+query+"&quarter=Q1", "", "analytics-key"); w.Code != 400 {
		t.Fatal("Clarityacceptedquarter")
	}
	if w := request("GET", "/api/v1/marketing/ga4"+query+"&quarter=Q9", "", "analytics-key"); w.Code != 400 {
		t.Fatal("invalidquarteraccepted")
	}
	for _, bad := range []string{"?site=%zz&websiteId=" + website.ID, query + "&quarter=Q1&quarter=Q2", strings.Replace(query, "example.com", "localhost", 1), strings.Replace(query, website.ID, "../invalid", 1)} {
		if w := request("GET", "/api/v1/marketing/ga4"+bad, "", "analytics-key"); w.Code != 400 {
			t.Fatalf("malformed query accepted: %d", w.Code)
		}
	}
	for range 121 {
		request("GET", "/api/v1/marketing/ga4"+query, "", "analytics-key")
	}
	if w := request("GET", "/api/v1/marketing/ga4"+query, "", "analytics-key"); w.Code != 429 {
		t.Fatal("analytics reads are not rate bounded")
	}
}

func TestAnalyticsUnavailableFallbackDoesNotInventMetrics(t *testing.T) {
	registry := newAnalyticsRegistry(t)
	website, _, err := registry.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	handler := NewHandler("key", registrySource{registry})
	for _, path := range []string{"ga4", "clarity"} {
		r := httptest.NewRequest("GET", "/api/v1/marketing/"+path+"?websiteId="+website.ID+"&site=https%3A%2F%2Fexample.com%2F", nil)
		r.Header.Set("X-Internal-API-Key", "key")
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		if w.Code != 200 || !strings.Contains(w.Body.String(), `"reason":"not_configured"`) || strings.Contains(w.Body.String(), `"sessions":0`) {
			t.Fatalf("fallback: %d %s", w.Code, w.Body.String())
		}
	}
}
