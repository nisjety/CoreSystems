package siteintegrations

import (
	"encoding/json"
	"net/http"
	"testing"
)

func TestDiscoveryResolvesGA4AndSearchConsoleWWWAliases(t *testing.T) {
	for _, site := range []string{"https://aquatiq.com/", "https://www.aquatiq.com/"} {
		broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
			if r.URL.Path == "/api/v1/connections" {
				writeFixture(w, googleConnections([]string{gscScope, gaScope}, []string{"search_console.read", "analytics.read"}))
				return
			}
			var a struct {
				Operation string `json:"operation"`
			}
			_ = json.NewDecoder(r.Body).Decode(&a)
			switch a.Operation {
			case "gsc.sites.list":
				writeAction(w, a.Operation, map[string]any{"siteEntry": []any{map[string]string{"siteUrl": "https://www.aquatiq.com/", "permissionLevel": "siteFullUser"}}})
			case "ga4.account_summaries.list":
				writeAction(w, a.Operation, map[string]any{"accountSummaries": []any{map[string]any{"propertySummaries": []any{map[string]string{"property": "properties/123"}}}}})
			case "ga4.data_streams.list":
				writeAction(w, a.Operation, map[string]any{"dataStreams": []any{map[string]any{"name": "properties/123/dataStreams/456", "type": "WEB_DATA_STREAM", "webStreamData": map[string]string{"defaultUri": "https://aquatiq.com/", "measurementId": "G-ABC123"}}}})
			case "ga4.reporting_host.resolve":
				writeAction(w, a.Operation, map[string]any{"reportingSite": "https://www.aquatiq.com/"})
			default:
				t.Fatal("unexpected operation", a.Operation)
			}
		})
		got := broker.Discover(t.Context(), site)
		if got.SearchConsole.Status != "connected" || got.SearchConsole.Property != "https://www.aquatiq.com/" || got.GA4.Status != "connected" {
			t.Fatal(got)
		}
		if site == "https://aquatiq.com/" && got.GA4.ReportingSite != "https://www.aquatiq.com/" {
			t.Fatal("resolved host not carried to mapping")
		}
	}
}
