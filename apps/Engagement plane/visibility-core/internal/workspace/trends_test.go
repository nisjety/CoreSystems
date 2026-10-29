package workspace

import (
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/trends"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestTrendsBoundToSiteRevisionAndIntegrationOrganization(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	site, _, _ := registry.Add(t.Context(), "https://triodelab.no/")
	query := trends.Query{Terms: []string{"web design"}, Geo: "NO", Timeframe: "today 12-m"}
	calls := 0
	retarget := false
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.URL.Path != "/api/v1/insights/trends" || r.Header.Get("Authorization") != "Bearer "+syntheticBearer {
			t.Error("wrong authority")
		}
		var input trends.Query
		if json.NewDecoder(r.Body).Decode(&input) != nil || input.Terms[0] != "web design" {
			t.Error("wrong input")
		}
		if retarget {
			_, _ = registry.MapClarity(site.ID, "abc123", site.Revision)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": trends.Report{Query: query, Source: "pytrends_dev", Scale: "relative_0_100_per_request", Status: "no_data", ObservedAt: time.Now().UTC().Format(time.RFC3339), Rows: []trends.Row{}}})
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	s := New(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}, registry)
	if r, e := s.GetTrends(t.Context(), site.ID, site.Site, site.Revision, query); e != nil || r.Status != "no_data" {
		t.Fatal(e)
	}
	before := calls
	if _, e := s.GetTrends(t.Context(), site.ID, "https://other.no/", site.Revision, query); e == nil || calls != before {
		t.Fatal("wrong site reached provider")
	}
	if _, e := s.GetTrends(t.Context(), site.ID, site.Site, site.Revision+1, query); e == nil || calls != before {
		t.Fatal("stale revision reached provider")
	}
	retarget = true
	if _, e := s.GetTrends(t.Context(), site.ID, site.Site, site.Revision, query); e == nil {
		t.Fatal("changed context accepted")
	}
}
