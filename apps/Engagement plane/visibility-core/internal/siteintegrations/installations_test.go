package siteintegrations

import (
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http"
	"testing"
)

func TestInstallationDiscoveryIsOnlyScopedMetadata(t *testing.T) {
	w := websites.Website{ID: "web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Site: "https://triodelab.no/", Revision: 4}
	broker := fixtureBroker(t, func(response http.ResponseWriter, request *http.Request) {
		if request.Method != "POST" || (request.URL.Path != "/api/v1/insights/site-sources/binding" && request.URL.Path != "/api/v1/insights/web-vitals/binding") {
			t.Fatal("discovery fetched provider data")
		}
		var body map[string]any
		_ = json.NewDecoder(request.Body).Decode(&body)
		if body["websiteId"] != w.ID || body["site"] != w.Site || body["websiteRevision"] != float64(4) || body["orgId"] != nil || body["apiKey"] != nil {
			t.Fatal("incorrect scope", body)
		}
		writeFixture(response, map[string]any{"success": true, "data": map[string]any{"configured": true}})
	})
	for _, key := range []string{"cookiebot", "algolia", "web_vitals"} {
		ok, err := broker.InstallationConfigured(context.Background(), w, key)
		if err != nil || !ok {
			t.Fatal(key, ok, err)
		}
	}
}
