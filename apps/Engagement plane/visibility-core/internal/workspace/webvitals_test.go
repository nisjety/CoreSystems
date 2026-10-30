package workspace

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/webvitals"
)

func TestWebVitalsCollectorOnlyAuthorizedFixedIntegrationRoute(t *testing.T) {
	registry := workspaceRegistry(t, func(context.Context, string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "conn-a", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}}
	})
	site, _, _ := registry.Add(t.Context(), "https://triodelab.no/")
	record, _ := registry.Resolve(site.ID, site.Site)
	mode := "good"
	calls := 0
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.Method != "POST" || r.URL.Path != "/api/v1/insights/web-vitals" || r.Header.Get("Authorization") != "Bearer "+syntheticBearer {
			t.Error("wrong boundary")
		}
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if len(body) != 4 || body["websiteId"] != site.ID || body["site"] != site.Site || body["environment"] != "production" || body["websiteRevision"] != float64(site.Revision) {
			t.Error(body)
		}
		switch mode {
		case "redirect":
			http.Redirect(w, r, "https://evil.example", 302)
			return
		case "status":
			w.WriteHeader(503)
			return
		case "oversize":
			_, _ = w.Write([]byte(strings.Repeat("x", (2<<20)+1)))
			return
		case "unknown":
			_, _ = w.Write([]byte(`{"success":true,"data":{},"token":"not-allowed"}`))
			return
		case "retarget":
			_, _ = registry.MapClarity(site.ID, "abc123", site.Revision)
		}
		data := webvitals.Export{WebsiteID: site.ID, Site: site.Site, WebsiteRevision: site.Revision, Environment: "production", Status: "connected", ReportingOrigin: "https://triodelab.no", ObservedAt: time.Now().UTC().Format(time.RFC3339Nano), Samples: []webvitals.Sample{}}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": data})
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	cfg := pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}
	c := NewWebVitalsCollector(cfg, registry)
	if _, err := c.Collect(t.Context(), record, "production"); err != nil {
		t.Fatal(err)
	}
	for _, m := range []string{"redirect", "status", "oversize", "unknown", "retarget"} {
		mode = m
		if _, err := c.Collect(t.Context(), record, "production"); err == nil {
			t.Fatal(m)
		}
	}
	prior := calls
	if _, err := c.Collect(t.Context(), record, "production"); err == nil || calls != prior {
		t.Fatal("stale authority used")
	}
	service := New(cfg, registry)
	if service.WithWebVitals(nil) == service || service.WebVitals() != nil {
		t.Fatal("mutated service")
	}
}
