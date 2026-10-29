package pulse

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func clarityFixtureService(t *testing.T, status int, mutate func(map[string]any)) *Service {
	t.Helper()
	data := map[string]any{"site": "https://example.com/", "source": "microsoft_clarity", "status": "connected", "scope": "clarity_project", "projectId": "abc123", "window": map[string]any{"numOfDays": 3, "timeZone": "UTC", "observedAt": "2026-10-21T12:00:00Z"}, "metrics": map[string]any{"sessions": 12, "botSessions": 1, "users": 8, "pagesPerSession": 1.25}, "updatedAt": "2026-10-21T12:00:00Z", "rawToken": "private-token"}
	if mutate != nil {
		mutate(data)
	}
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "GET" || r.URL.Path != "/api/v1/insights/clarity" || r.URL.Query().Get("site") != "https://example.com/" || r.URL.Query().Get("numOfDays") != "3" || len(r.URL.Query()) != 2 || r.Header.Get("Authorization") != "Bearer synthetic" {
			t.Error("unsafe Clarity request")
		}
		w.WriteHeader(status)
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": data})
	}))
	t.Cleanup(provider.Close)
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte("synthetic"), 0600); err != nil {
		t.Fatal(err)
	}
	return NewService(Config{IntegrationCoreURL: provider.URL, IntegrationBearerTokenFile: path, ClarityProjectID: "abc123"}, provider.Client(), nil)
}
func TestClarityUsesApprovedBrokerProjectAndOnlyTypedAggregates(t *testing.T) {
	got := clarityFixtureService(t, 200, nil).GetClarity(t.Context(), "https://example.com/")
	if got.Status != "connected" || got.Metrics == nil || got.Metrics.Sessions == nil || *got.Metrics.Sessions != 12 || got.Window.NumOfDays != 3 || got.Window.TimeZone != "UTC" || got.Scope != "clarity_project" {
		t.Fatalf("projection: %+v", got)
	}
	raw, _ := json.Marshal(got)
	if strings.Contains(string(raw), "private-token") || strings.Contains(string(raw), "rawToken") {
		t.Fatal("private provider output escaped")
	}
}
func TestClarityRejectsWrongScopeProjectSiteAndMalformedMetrics(t *testing.T) {
	for _, mode := range []string{"site", "project", "missing project", "invalid project", "scope", "days", "timezone", "count", "negative count", "unsafe count", "negative", "unsafe ratio", "strategy", "timestamp", "all null", "metrics with unavailable", "unknown reason", "connected reason"} {
		t.Run(mode, func(t *testing.T) {
			got := clarityFixtureService(t, 200, func(v map[string]any) {
				switch mode {
				case "site":
					v["site"] = "https://other.example/"
				case "project":
					v["projectId"] = "other123"
				case "missing project":
					delete(v, "projectId")
				case "invalid project":
					v["projectId"] = "../project"
				case "scope":
					v["scope"] = "site"
				case "days":
					v["window"].(map[string]any)["numOfDays"] = 90
				case "timezone":
					v["window"].(map[string]any)["timeZone"] = "Europe/Oslo"
				case "count":
					v["metrics"].(map[string]any)["sessions"] = 1.5
				case "negative count":
					v["metrics"].(map[string]any)["sessions"] = -1
				case "unsafe count":
					v["metrics"].(map[string]any)["sessions"] = 9007199254740992
				case "negative":
					v["metrics"].(map[string]any)["pagesPerSession"] = -1
				case "unsafe ratio":
					v["metrics"].(map[string]any)["pagesPerSession"] = 1e100
				case "strategy":
					v["status"] = "madeup"
				case "timestamp":
					v["updatedAt"] = "tomorrow"
				case "all null":
					v["metrics"] = map[string]any{"sessions": nil, "botSessions": nil, "users": nil, "pagesPerSession": nil}
				case "metrics with unavailable":
					v["status"] = "unavailable"
					v["reason"] = "quota_limited"
				case "unknown reason":
					v["status"] = "unavailable"
					v["reason"] = "private-secret"
					v["metrics"] = nil
				case "connected reason":
					v["reason"] = "private-secret"
				}
			}).GetClarity(t.Context(), "https://example.com/")
			if got.Status != "unavailable" || got.Metrics != nil || got.Reason != "source_unavailable" {
				t.Fatalf("untrusted projection accepted: %+v", got)
			}
		})
	}
}
func TestClarityPreservesHonestNullsAndUnavailableReasons(t *testing.T) {
	got := clarityFixtureService(t, 200, func(v map[string]any) { v["metrics"].(map[string]any)["users"] = nil }).GetClarity(t.Context(), "https://example.com/")
	if got.Metrics == nil || got.Metrics.Users != nil {
		t.Fatal("missing users became measured zero")
	}
	for _, reason := range []string{"not_connected", "pending_verification", "not_configured", "quota_limited", "source_unavailable", "authorization_required", "no_data"} {
		got := clarityFixtureService(t, 200, func(v map[string]any) { v["status"] = "unavailable"; v["reason"] = reason; v["metrics"] = nil }).GetClarity(t.Context(), "https://example.com/")
		if got.Status != "unavailable" || got.Reason != reason || got.Metrics != nil {
			t.Fatalf("reason lost: %+v", got)
		}
	}
	if got := clarityFixtureService(t, 500, nil).GetClarity(t.Context(), "https://example.com/"); got.Reason != "source_unavailable" || got.Metrics != nil {
		t.Fatal("provider failure became data")
	}
	if got := NewService(Config{}, nil, nil).GetClarity(t.Context(), "https://example.com/"); got.Reason != "not_configured" {
		t.Fatal(got)
	}
	if got := NewService(Config{}, nil, nil).GetClarity(t.Context(), "https://127.0.0.1/"); got.Reason != "source_unavailable" {
		t.Fatal("unsafe site accepted")
	}
}
