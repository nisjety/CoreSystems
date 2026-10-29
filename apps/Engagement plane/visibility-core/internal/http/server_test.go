package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type fakePulse struct {
	quarter string
	site    string
	result  pulse.Projection
}

func (f *fakePulse) Get(_ context.Context, quarter, site string) pulse.Projection {
	f.quarter = quarter
	f.site = site
	return f.result
}

func TestMarketingLandingPageIsPublicStaticHTML(t *testing.T) {
	handler := NewHandler("internal-test-key")
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, httptest.NewRequest(http.MethodGet, "/", nil))

	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200", recorder.Code)
	}
	if got := recorder.Header().Get("Content-Type"); !strings.HasPrefix(got, "text/html; charset=utf-8") {
		t.Fatalf("Content-Type = %q, want HTML", got)
	}
	for header, want := range map[string]string{
		"Content-Security-Policy": "default-src 'none'",
		"X-Content-Type-Options":  "nosniff",
		"X-Frame-Options":         "DENY",
	} {
		if got := recorder.Header().Get(header); !strings.Contains(got, want) {
			t.Errorf("%s = %q, want %q", header, got, want)
		}
	}
	body := recorder.Body.String()
	for _, expected := range []string{"<title>Visibility Core</title>", "<main>", "<h1>Visibility Core</h1>"} {
		if !strings.Contains(body, expected) {
			t.Errorf("landing page missing %q: %s", expected, body)
		}
	}
	for _, forbidden := range []string{"<script", "<form", "access_token", "refresh_token", "client_secret", "api_key", "connectionId", "internal-test-key"} {
		if strings.Contains(strings.ToLower(body), strings.ToLower(forbidden)) {
			t.Errorf("landing page leaked or enabled %q: %s", forbidden, body)
		}
	}
}

func TestMarketingHealthAndReadyArePublicJSON(t *testing.T) {
	handler := NewHandler("internal-test-key")
	for _, path := range []string{"/health", "/ready"} {
		recorder := httptest.NewRecorder()
		handler.ServeHTTP(recorder, httptest.NewRequest(http.MethodGet, path, nil))
		if recorder.Code != http.StatusOK {
			t.Errorf("%s status = %d, want 200", path, recorder.Code)
		}
		if got := recorder.Header().Get("Content-Type"); !strings.HasPrefix(got, "application/json") {
			t.Errorf("%s Content-Type = %q, want JSON", path, got)
		}
		var body map[string]string
		if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
			t.Errorf("%s JSON: %v", path, err)
		}
		if body["status"] != "ok" || body["service"] != "visibility-core" {
			t.Errorf("%s body = %#v, want visibility-core ok", path, body)
		}
	}
}

func TestVisibilityAndLegacyStatusShareAuthentication(t *testing.T) {
	handler := NewHandler("internal-test-key")
	for _, path := range []string{"/api/v1/visibility/status", "/api/v1/marketing/status"} {
		unauthorized := httptest.NewRecorder()
		handler.ServeHTTP(unauthorized, httptest.NewRequest(http.MethodGet, path, nil))
		if unauthorized.Code != http.StatusUnauthorized {
			t.Fatalf("%s without key = %d", path, unauthorized.Code)
		}
		request := httptest.NewRequest(http.MethodGet, path, nil)
		request.Header.Set("X-Internal-API-Key", "internal-test-key")
		authorized := httptest.NewRecorder()
		handler.ServeHTTP(authorized, request)
		if authorized.Code != http.StatusOK || !strings.Contains(authorized.Body.String(), `"service":"visibility-core"`) {
			t.Fatalf("%s = %d %s", path, authorized.Code, authorized.Body.String())
		}
	}
}

func TestMarketingUnknownAPIRouteDoesNotFallBackToLandingPage(t *testing.T) {
	handler := NewHandler("internal-test-key")
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, httptest.NewRequest(http.MethodGet, "/api/v1/marketing/unknown", nil))

	if recorder.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404; body=%s", recorder.Code, recorder.Body.String())
	}
	if strings.Contains(recorder.Body.String(), "Visibility Core") {
		t.Fatalf("unknown API route returned landing page: %s", recorder.Body.String())
	}
}

func TestPulseRouteRequiresInternalKeyAndReturnsOnlyProjection(t *testing.T) {
	service := &fakePulse{result: pulse.Projection{
		Site: "https://aquatiq.com/", Source: "google_search_console", Status: pulse.StatusConnected,
		Period: pulse.Period{StartDate: "2026-10-01", EndDate: "2026-10-20"},
		Totals: &pulse.Totals{Clicks: 3, Impressions: 20},
		Days:   []pulse.Day{{Date: "2026-10-01", Clicks: 3, Impressions: 20}},
	}}
	handler := NewHandler("internal-test-key", service)
	path := "/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2Faquatiq.com%2F"
	unauthorized := httptest.NewRecorder()
	handler.ServeHTTP(unauthorized, httptest.NewRequest(http.MethodGet, path, nil))
	if unauthorized.Code != http.StatusUnauthorized || service.quarter != "" {
		t.Fatalf("unauthorized=%d called=%q", unauthorized.Code, service.quarter)
	}
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.Header.Set("X-Internal-API-Key", "internal-test-key")
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, req)
	if response.Code != http.StatusOK || service.quarter != "Q1" || service.site != "https://aquatiq.com/" {
		t.Fatalf("status=%d quarter=%q site=%q body=%s", response.Code, service.quarter, service.site, response.Body.String())
	}
	var body struct {
		Data pulse.Projection `json:"data"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || body.Data.Totals == nil || body.Data.Totals.Clicks != 3 {
		t.Fatalf("body=%s err=%v", response.Body.String(), err)
	}
	if got := response.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control=%q", got)
	}
}

func TestPulseRouteRejectsInvalidQuarterMethodAndExtraQuery(t *testing.T) {
	service := &fakePulse{}
	handler := NewHandler("internal-test-key", service)
	for _, tc := range []struct {
		method string
		path   string
		want   int
	}{
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q5&site=https%3A%2F%2Faquatiq.com%2F", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1&site=other.example", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2Faquatiq.com%2Fpath", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2Flocalhost%2F", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2F127.0.0.1%2F", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2Faquatiq.com%3A443%2F", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2Faquatiq.com%2F&site=https%3A%2F%2Fother.example%2F", http.StatusBadRequest},
		{http.MethodGet, "/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2Faquatiq.com%2F&extra=1", http.StatusBadRequest},
		{http.MethodPost, "/api/v1/marketing/pulse", http.StatusMethodNotAllowed},
	} {
		req := httptest.NewRequest(tc.method, tc.path, nil)
		req.Header.Set("X-Internal-API-Key", "internal-test-key")
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != tc.want || service.quarter != "" {
			t.Errorf("%s %s = %d, called=%q body=%s", tc.method, tc.path, response.Code, service.quarter, response.Body.String())
		}
	}
}
