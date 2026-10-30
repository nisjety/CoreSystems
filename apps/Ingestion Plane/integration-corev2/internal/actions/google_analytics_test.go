package actions

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestGoogleAnalyticsReadActionsUseFixedAdminEndpoints(t *testing.T) {
	for _, tc := range []struct {
		operation, path string
		params          map[string]any
	}{
		{"ga4.account_summaries.list", "/v1beta/accountSummaries?pageSize=100", nil},
		{"ga4.data_streams.list", "/v1beta/properties/123/dataStreams?pageSize=50&pageToken=opaque-token", map[string]any{"propertyId": "123", "pageSize": float64(50), "pageToken": "opaque-token"}},
	} {
		t.Run(tc.operation, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Method != "GET" || r.URL.RequestURI() != tc.path || r.Header.Get("Authorization") != "Bearer synthetic" {
					t.Errorf("unexpected provider request %s %s", r.Method, r.URL.RequestURI())
				}
				_, _ = w.Write([]byte(`{"nextPageToken":"next"}`))
			}))
			defer server.Close()
			service := NewService(config.Config{GoogleAnalyticsAdminAPIBaseURL: server.URL}, server.Client())
			if _, err := service.Execute(context.Background(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, AccessToken: "synthetic", Operation: tc.operation, Params: tc.params}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestGoogleAnalyticsReadActionsRejectUnsafeParametersBeforeProviderCall(t *testing.T) {
	for _, tc := range []struct {
		operation    string
		params, body map[string]any
	}{
		{"ga4.data_streams.list", map[string]any{"propertyId": "../1"}, nil},
		{"ga4.data_streams.list", map[string]any{"propertyId": "properties/1"}, nil},
		{"ga4.data_streams.list", map[string]any{"propertyId": "1", "url": "https://evil.example"}, nil},
		{"ga4.account_summaries.list", map[string]any{"pageSize": 201}, nil},
		{"ga4.account_summaries.list", map[string]any{"pageSize": 1.2}, nil},
		{"ga4.account_summaries.list", map[string]any{"pageToken": strings.Repeat("a", 2049)}, nil},
		{"ga4.account_summaries.list", map[string]any{"pageToken": "bad\nvalue"}, nil},
		{"ga4.account_summaries.list", nil, map[string]any{"create": true}},
	} {
		if _, err := NewService(config.Config{}, nil).Execute(context.Background(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, Operation: tc.operation, Params: tc.params, Body: tc.body}); err == nil {
			t.Errorf("accepted unsafe params %#v", tc.params)
		}
	}
}
