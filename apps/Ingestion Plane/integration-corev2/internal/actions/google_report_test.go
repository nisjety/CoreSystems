package actions

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestGA4ReportVerifiesStreamAndUsesFixedHostScopedSummaryAndDailyReports(t *testing.T) {
	posts := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer synthetic" {
			t.Error("missing provider bearer")
		}
		if r.Method == "GET" {
			if r.URL.Path != "/v1beta/properties/123/dataStreams/456" {
				t.Errorf("unsafe streamlookup %s", r.URL.Path)
			}
			_, _ = w.Write([]byte(`{"name":"properties/123/dataStreams/456","type":"WEB_DATA_STREAM","webStreamData":{"defaultUri":"https://aquatiq.com"}}`))
			return
		}
		if r.URL.Path != "/v1beta/properties/123:runReport" {
			t.Errorf("unsafe reportendpoint %s", r.URL.Path)
		}
		var report map[string]any
		if err := json.NewDecoder(r.Body).Decode(&report); err != nil {
			t.Fatal(err)
		}
		encoded, _ := json.Marshal(report)
		for _, required := range []string{`"fieldName":"hostName"`, `"value":"aquatiq.com"`, `"fieldName":"streamId"`, `"value":"456"`, `"name":"totalUsers"`, `"name":"userEngagementDuration"`} {
			if !strings.Contains(string(encoded), required) {
				t.Errorf("missing constraint %s", required)
			}
		}
		if posts == 0 {
			if _, exists := report["dimensions"]; exists {
				t.Error("summary must not sum daily unique users")
			}
		} else if !strings.Contains(string(encoded), `"name":"date"`) {
			t.Error("missing daily date dimension")
		}
		posts++
		_, _ = w.Write([]byte(`{"rows":[],"metadata":{"timeZone":"Europe/Oslo"}}`))
	}))
	defer server.Close()
	service := NewService(config.Config{GoogleAnalyticsAdminAPIBaseURL: server.URL, GoogleAnalyticsDataAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(t.Context(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, AccessToken: "synthetic", Operation: "ga4.report.run", Params: map[string]any{"site": "https://aquatiq.com/", "propertyId": "123", "streamId": "456"}, Body: map[string]any{"startDate": "2026-01-01", "endDate": "2026-01-31"}})
	if err != nil || posts != 2 {
		t.Fatalf("report error=%v posts=%d", err, posts)
	}
	object, ok := result.Result.(map[string]any)
	if !ok || object["summary"] == nil || object["daily"] == nil {
		t.Fatalf("report=%#v", result.Result)
	}
}

func TestGA4ReportRejectsUnsafeInputBeforeFetchingProvider(t *testing.T) {
	baseParams := map[string]any{"site": "https://aquatiq.com/", "propertyId": "123", "streamId": "456"}
	baseBody := map[string]any{"startDate": "2026-01-01", "endDate": "2026-01-31"}
	for _, tc := range []struct{ params, body map[string]any }{
		{map[string]any{"site": "https://aquatiq.com/", "propertyId": "../bad", "streamId": "456"}, baseBody},
		{map[string]any{"site": "https://localhost/", "propertyId": "123", "streamId": "456"}, baseBody},
		{map[string]any{"site": "https://aquatiq.com/", "propertyId": "123", "streamId": "456", "url": "https://evil.example"}, baseBody},
		{baseParams, map[string]any{"startDate": "2026-02-31", "endDate": "2026-03-03"}},
		{baseParams, map[string]any{"startDate": "2026-02-01", "endDate": "2026-01-01"}},
		{baseParams, map[string]any{"startDate": "2024-01-01", "endDate": "2026-01-01"}},
		{baseParams, map[string]any{"startDate": "2099-01-01", "endDate": "2099-01-31"}},
		{baseParams, map[string]any{"startDate": "2026-01-01", "endDate": "2026-01-31", "metrics": []string{"secret"}}},
	} {
		if _, err := NewService(config.Config{}, nil).Execute(context.Background(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, Operation: "ga4.report.run", Params: tc.params, Body: tc.body}); err == nil {
			t.Error("accepted unsafe GA4 report")
		}
	}
	if _, err := analyticsReportDates(baseBody, time.Date(2026, 2, 1, 0, 0, 0, 0, time.UTC)); err != nil {
		t.Fatal(err)
	}
}

func TestGA4ReportDeniesMismatchedStreamAndRedactsSourceErrors(t *testing.T) {
	for _, response := range []string{`{"name":"properties/123/dataStreams/456","type":"WEB_DATA_STREAM","webStreamData":{"defaultUri":"https://www.aquatiq.com/"}}`, `{"name":"properties/999/dataStreams/456","type":"WEB_DATA_STREAM","webStreamData":{"defaultUri":"https://aquatiq.com/"}}`, `{"secret":"synthetic-private-value"}`} {
		posts := 0
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.Method == "POST" {
				posts++
			}
			_, _ = w.Write([]byte(response))
		}))
		_, err := NewService(config.Config{GoogleAnalyticsAdminAPIBaseURL: server.URL}, server.Client()).Execute(t.Context(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, Operation: "ga4.report.run", Params: map[string]any{"site": "https://aquatiq.com/", "propertyId": "123", "streamId": "456"}, Body: map[string]any{"startDate": "2026-01-01", "endDate": "2026-01-31"}})
		server.Close()
		if err == nil || posts != 0 || strings.Contains(err.Error(), "synthetic-private") {
			t.Fatalf("unsafe provider result err=%v posts=%d", err, posts)
		}
	}
}

func TestGA4ReportProviderFailuresNeverExposeResponseBody(t *testing.T) {
	for _, failAt := range []int{0, 1, 2} {
		calls := 0
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			current := calls
			calls++
			if current == failAt {
				w.WriteHeader(403)
				_, _ = w.Write([]byte(`{"error":"synthetic-private-provider-error"}`))
				return
			}
			if r.Method == "GET" {
				_, _ = w.Write([]byte(`{"name":"properties/123/dataStreams/456","type":"WEB_DATA_STREAM","webStreamData":{"defaultUri":"https://aquatiq.com/"}}`))
				return
			}
			_, _ = w.Write([]byte(`{"rows":[]}`))
		}))
		_, err := NewService(config.Config{GoogleAnalyticsAdminAPIBaseURL: server.URL, GoogleAnalyticsDataAPIBaseURL: server.URL}, server.Client()).Execute(t.Context(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, Operation: "ga4.report.run", Params: map[string]any{"site": "https://aquatiq.com/", "propertyId": "123", "streamId": "456"}, Body: map[string]any{"startDate": "2026-01-01", "endDate": "2026-01-31"}})
		server.Close()
		if err == nil || strings.Contains(err.Error(), "synthetic-private") {
			t.Fatal("providererrorbodyescapedfixedreportboundary")
		}
	}
}

func TestGA4ReportSupportsOnlyExplicitWWWReportingSite(t *testing.T) {
	posts, gets := 0, 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == "GET" {
			gets++
			if r.URL.Path == "/webmasters/v3/sites" {
				_, _ = w.Write([]byte(`{"siteEntry":[{"siteUrl":"sc-domain:triodelab.no","permissionLevel":"siteOwner"}]}`))
				return
			}
			_, _ = w.Write([]byte(`{"name":"properties/560102923/dataStreams/16177443323","type":"WEB_DATA_STREAM","webStreamData":{"defaultUri":"https://triodelab.no/"}}`))
			return
		}
		posts++
		var report map[string]any
		if err := json.NewDecoder(r.Body).Decode(&report); err != nil {
			t.Fatal(err)
		}
		encoded, _ := json.Marshal(report)
		if !strings.Contains(string(encoded), `"fieldName":"hostName","stringFilter":{"caseSensitive":false,"matchType":"EXACT","value":"www.triodelab.no"}`) || !strings.Contains(string(encoded), `"fieldName":"streamId"`) {
			t.Errorf("missing exact filters: %s", encoded)
		}
		_, _ = w.Write([]byte(`{"rows":[]}`))
	}))
	defer server.Close()
	service := NewService(config.Config{GoogleAPIBaseURL: server.URL, GoogleAnalyticsAdminAPIBaseURL: server.URL, GoogleAnalyticsDataAPIBaseURL: server.URL}, server.Client())
	input := ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, AccessToken: "synthetic", Operation: "ga4.report.run", Params: map[string]any{"site": "https://triodelab.no/", "reportingSite": "https://www.triodelab.no/", "propertyId": "560102923", "streamId": "16177443323"}, Body: map[string]any{"startDate": "2026-01-01", "endDate": "2026-01-31"}}
	if _, err := service.Execute(t.Context(), input); err != nil || posts != 2 || gets != 1 {
		t.Fatalf("explicit www report: %v posts=%d gets=%d", err, posts, gets)
	}
	for _, target := range []string{"https://evil.example/", "https://blog.triodelab.no/", "https://www.triodelab.no/path", "https://www.triodelab.no:443/", "https://www.www.triodelab.no/"} {
		params := map[string]any{"site": "https://triodelab.no/", "reportingSite": target, "propertyId": "560102923", "streamId": "16177443323"}
		if _, err := service.Execute(t.Context(), ExecuteInput{Connection: input.Connection, AccessToken: input.AccessToken, Operation: input.Operation, Params: params, Body: input.Body}); err == nil || posts != 2 || gets != 1 {
			t.Fatalf("unsafe target %q: err=%v posts=%d gets=%d", target, err, posts, gets)
		}
	}
}

func TestGA4ReportAliasUsesAnalyticsResourcePermissionNotSearchConsoleOwnership(t *testing.T) {
	for _, tc := range []struct {
		name, response string
		status         int
	}{
		{"prefix-owner", `{"siteEntry":[{"siteUrl":"https://triodelab.no/","permissionLevel":"siteOwner"}]}`, 200},
		{"domain-reader", `{"siteEntry":[{"siteUrl":"sc-domain:triodelab.no","permissionLevel":"siteFullUser"}]}`, 200},
		{"revoked", `{"siteEntry":[]}`, 200},
		{"unavailable", `{"error":"private"}`, 403},
	} {
		t.Run(tc.name, func(t *testing.T) {
			posts := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.URL.Path == "/webmasters/v3/sites" {
					t.Error("GA4 read permission must not depend on a separate GSC grant")
					w.WriteHeader(tc.status)
					_, _ = w.Write([]byte(tc.response))
					return
				}
				if r.Method == "POST" {
					posts++
				}
				_, _ = w.Write([]byte(`{"name":"properties/560102923/dataStreams/16177443323","type":"WEB_DATA_STREAM","webStreamData":{"defaultUri":"https://triodelab.no/"}}`))
			}))
			defer server.Close()
			service := NewService(config.Config{GoogleAPIBaseURL: server.URL, GoogleAnalyticsAdminAPIBaseURL: server.URL, GoogleAnalyticsDataAPIBaseURL: server.URL}, server.Client())
			_, err := service.Execute(t.Context(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, AccessToken: "synthetic", Operation: "ga4.report.run", Params: map[string]any{"site": "https://triodelab.no/", "reportingSite": "https://www.triodelab.no/", "propertyId": "560102923", "streamId": "16177443323"}, Body: map[string]any{"startDate": "2026-01-01", "endDate": "2026-01-31"}})
			if err != nil || posts != 2 {
				t.Fatalf("unsafe alternate: err=%v posts=%d", err, posts)
			}
		})
	}
}
