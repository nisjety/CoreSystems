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
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func TestSiteSourcesCollectorSignedFixedIntegrationBoundary(t *testing.T) {
	r := workspaceRegistry(t, func(context.Context, string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "conn-a", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}}
	})
	site, _, _ := r.Add(t.Context(), "https://triodelab.no/")
	record, _ := r.Resolve(site.ID, site.Site)
	mode := "good"
	calls := 0
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		calls++
		if req.Method != "POST" || req.URL.Path != "/api/v1/insights/site-sources" || req.Header.Get("Authorization") != "Bearer "+syntheticBearer {
			t.Error("wrong boundary")
		}
		var body map[string]any
		_ = json.NewDecoder(req.Body).Decode(&body)
		if len(body) != 4 || body["websiteId"] != site.ID || body["site"] != site.Site || body["websiteRevision"] != float64(site.Revision) || body["provider"] != "algolia" {
			t.Error(body)
		}
		switch mode {
		case "redirect":
			http.Redirect(w, req, "https://evil.example", 302)
			return
		case "status":
			w.WriteHeader(503)
			return
		case "oversize":
			_, _ = w.Write([]byte(strings.Repeat("x", sitesources.MaxReceiptBytes+1)))
			return
		case "unknown":
			_, _ = w.Write([]byte(`{"success":true,"data":{},"apiKey":"bad"}`))
			return
		case "retarget":
			_, _ = r.MapClarity(site.ID, "abc123", site.Revision)
		}
		now := time.Now().UTC()
		day := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
		report := sitesources.Report{WebsiteID: site.ID, Site: site.Site, WebsiteRevision: site.Revision, Provider: "algolia", Status: "not_configured", ObservedAt: now.Format(time.RFC3339Nano), Window: sitesources.Window{StartDate: day.AddDate(0, 0, -7).Format("2006-01-02"), EndDate: day.AddDate(0, 0, -1).Format("2006-01-02")}}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": report})
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	cfg := pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}
	collector := NewSiteSourcesCollector(cfg, r)
	if _, err := collector.Collect(t.Context(), record, "algolia"); err != nil {
		t.Fatal(err)
	}
	for _, m := range []string{"redirect", "status", "oversize", "unknown", "retarget"} {
		mode = m
		if _, err := collector.Collect(t.Context(), record, "algolia"); err == nil {
			t.Fatal(m)
		}
	}
	before := calls
	if _, err := collector.Collect(t.Context(), record, "algolia"); err == nil || calls != before {
		t.Fatal("stale authority used")
	}
	s := New(cfg, r)
	if s.WithSiteSources(nil) == s || s.SiteSources() != nil {
		t.Fatal("mutated workspace")
	}
}
