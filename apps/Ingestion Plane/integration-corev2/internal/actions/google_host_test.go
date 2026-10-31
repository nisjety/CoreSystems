package actions

import (
	"encoding/json"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestGoogleHostDiscoveryUsesOnlyAuthorizedStreamAndExactPair(t *testing.T) {
	for _, site := range []string{"https://triodelab.no/", "https://www.triodelab.no/"} {
		for _, selected := range []string{"triodelab.no", "www.triodelab.no", "foreign.no", ""} {
			posts := 0
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Method == "GET" {
					_, _ = w.Write([]byte(`{"name":"properties/123/dataStreams/456","type":"WEB_DATA_STREAM","webStreamData":{"defaultUri":"https://triodelab.no/"}}`))
					return
				}
				posts++
				var input map[string]any
				_ = json.NewDecoder(r.Body).Decode(&input)
				raw, _ := json.Marshal(input)
				for _, expected := range []string{`"fieldName":"streamId"`, `"value":"456"`, `"fieldName":"hostName"`, `"inListFilter"`, `"www.triodelab.no"`, `"triodelab.no"`, `"limit":"2"`} {
					if !strings.Contains(string(raw), expected) {
						t.Error("host discovery lost fixed resource filter", expected)
					}
				}
				rows := []any{}
				if selected != "" {
					rows = append(rows, map[string]any{"dimensionValues": []any{map[string]string{"value": selected}}, "metricValues": []any{map[string]string{"value": "5"}}})
				}
				_ = json.NewEncoder(w).Encode(map[string]any{"dimensionHeaders": []any{map[string]string{"name": "hostName"}}, "metricHeaders": []any{map[string]string{"name": "sessions"}}, "rows": rows, "rowCount": len(rows)})
			}))
			service := NewService(config.Config{GoogleAnalyticsAdminAPIBaseURL: server.URL, GoogleAnalyticsDataAPIBaseURL: server.URL}, server.Client())
			result, err := service.Execute(t.Context(), ExecuteInput{Connection: store.Connection{ProviderKey: "google"}, AccessToken: "synthetic", Operation: "ga4.reporting_host.resolve", Params: map[string]any{"site": site, "propertyId": "123", "streamId": "456"}})
			server.Close()
			if posts != 1 {
				t.Fatal("unbounded host discovery")
			}
			if selected == "foreign.no" {
				if err == nil {
					t.Fatal("foreign host accepted")
				}
				continue
			}
			if err != nil {
				t.Fatal(err)
			}
			expected := "https://" + selected + "/"
			if selected == "" {
				expected = "https://triodelab.no/"
			}
			if result.Result.(map[string]any)["reportingSite"] != expected {
				t.Fatal("wrong reporting origin", result)
			}
		}
	}
}
