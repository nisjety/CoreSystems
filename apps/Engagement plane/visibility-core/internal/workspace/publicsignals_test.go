package workspace

import (
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/publicsignals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestPublicSignalsCollectorBoundedIntegrationOnly(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	site, _, _ := registry.Add(t.Context(), "https://triodelab.no/")
	record, _ := registry.Resolve(site.ID, site.Site)
	mode := "good"
	calls := 0
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.Method != "POST" || r.URL.Path != "/api/v1/insights/public-signals" || r.Header.Get("Authorization") != "Bearer "+syntheticBearer {
			t.Error("unsaferequest")
		}
		var input map[string]string
		_ = json.NewDecoder(r.Body).Decode(&input)
		if len(input) != 1 || input["site"] != site.Site {
			t.Error("badbody")
		}
		switch mode {
		case "redirect":
			http.Redirect(w, r, "https://other.example/", 302)
			return
		case "status":
			w.WriteHeader(503)
			return
		case "oversize":
			_, _ = w.Write([]byte(strings.Repeat("x", 129<<10)))
			return
		case "malformed":
			_, _ = w.Write([]byte(`{"success":true,"data":{}}`))
			return
		case "retarget":
			_, _ = registry.MapClarity(site.ID, "abc123", site.Revision)
		}
		report := publicsignals.Report{Site: site.Site, Source: "public_web_signals", ObservedAt: time.Now().UTC().Format(time.RFC3339Nano), CrUX: []publicsignals.CrUX{{FormFactor: "PHONE", Status: "unavailable", Reason: "not_configured"}, {FormFactor: "DESKTOP", Status: "unavailable", Reason: "no_data"}}, CommonCrawl: publicsignals.CommonCrawl{Status: "unavailable", Reason: "no_data", Captures: []publicsignals.Capture{}, Limit: 20}}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": report})
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	cfg := pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}
	collector := NewPublicSignalsCollector(cfg, registry)
	got, err := collector.Collect(t.Context(), record)
	if err != nil || got.Site != site.Site {
		t.Fatal(got, err)
	}
	for _, value := range []string{"redirect", "status", "oversize", "malformed", "retarget"} {
		mode = value
		if _, err = collector.Collect(t.Context(), record); err == nil {
			t.Fatal("badresponseaccepted", mode)
		}
	}
	before := calls
	if _, err = collector.Collect(t.Context(), record); err == nil || calls != before {
		t.Fatal("oldrevisionreachedprovider")
	}
	if err = os.WriteFile(path, []byte("header.e30.signature"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = collector.Collect(t.Context(), record); err == nil || calls != before {
		t.Fatal("wrongorgreachedprovider")
	}
	service := New(cfg, registry)
	if service.WithPublicSignals(nil) == service || service.PublicSignals() != nil {
		t.Fatal("mutatingservice")
	}
}
