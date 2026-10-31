package workspace

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type deniedSnapshotAuthority struct{ calls atomic.Int64 }

func (a *deniedSnapshotAuthority) Check(context.Context, websites.Record) analyticssync.Access {
	a.calls.Add(1)
	return analyticssync.Access{}
}

func TestSnapshotAuthorityIsMetadataOnlyAndChecksRegistryContext(t *testing.T) {
	var calls atomic.Int64
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if r.Method != "GET" || r.URL.Path != "/api/v1/insights/clarity/binding" {
			t.Error("provider operation during authorization")
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": map[string]any{"site": "https://triodelab.no/", "configured": true, "projectId": "project123", "version": strings.Repeat("a", 64)}})
	}))
	defer upstream.Close()
	registry := workspaceRegistry(t, nil)
	w, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	record, _ := registry.Resolve(w.ID, w.Site)
	token := filepath.Join(t.TempDir(), "token")
	if err := os.WriteFile(token, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	authority := NewSnapshotAuthority(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: token}, registry)
	access := authority.Check(t.Context(), record)
	if !access.Clarity || access.ClarityProjectID != "project123" || access.SearchConsole || access.GA4 {
		t.Fatal(access)
	}
	stale := record
	stale.Website.Revision++
	if access := authority.Check(t.Context(), stale); access.Clarity {
		t.Fatal("stale registry context")
	}
	if calls.Load() != 1 {
		t.Fatal("stale context sent network request")
	}
	if err := os.WriteFile(token, []byte("invalid"), 0600); err != nil {
		t.Fatal(err)
	}
	if access := authority.Check(t.Context(), record); access.Clarity {
		t.Fatal("unscoped bearer")
	}
	if calls.Load() != 1 {
		t.Fatal("unscoped bearer sent network request")
	}
}

type fixedSnapshotAuthority struct{ access analyticssync.Access }

func (a fixedSnapshotAuthority) Check(context.Context, websites.Record) analyticssync.Access {
	return a.access
}
func TestSnapshotCollectorSkipsUnauthorizedProvidersAndCancellation(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	w, _, _ := registry.Add(t.Context(), "https://triodelab.no/")
	record, _ := registry.Resolve(w.ID, w.Site)
	source := New(pulse.Config{}, registry)
	for _, access := range []analyticssync.Access{{}, {SearchConsole: true, GA4: true, Clarity: true}} {
		collector := NewSnapshotCollector(source, fixedSnapshotAuthority{access})
		bundle, err := collector.Collect(t.Context(), record, currentSnapshotQuarter())
		if err != nil || bundle.Pulse.Totals != nil || bundle.GA4.Totals != nil || bundle.Clarity.Metrics != nil {
			t.Fatal("fabricated data", bundle, err)
		}
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		if _, err := collector.Collect(ctx, record, currentSnapshotQuarter()); err != context.Canceled {
			t.Fatal(err)
		}
	}
}
func TestSavedAnalyticsReadsNeverFallBackToLiveProviders(t *testing.T) {
	registry := workspaceRegistry(t, nil)
	w, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	store, err := analyticssync.Open(filepath.Join(dir, "analytics.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	jobs := analyticssync.New(store, registry, nil, &deniedSnapshotAuthority{}, nil)
	source := New(pulse.Config{IntegrationCoreURL: "http://127.0.0.1:1"}, registry).WithAnalyticsSync(jobs)
	if source.AnalyticsSync() != jobs {
		t.Fatal("wrong sync source")
	}
	if value := source.Get(t.Context(), "", w.Site); value.Status != "unavailable" || value.Reason != "snapshot_pending" || value.Totals != nil {
		t.Fatal("no saved pulse fallback", value)
	}
	if value := source.GetSearch(t.Context(), "", w.Site); value.Reason != "snapshot_pending" || len(value.Queries) != 0 {
		t.Fatal("no saved search fallback", value)
	}
	if value := source.GetGA4(t.Context(), "", w.Site); value.Reason != "snapshot_pending" || value.Totals != nil {
		t.Fatal("no saved GA4 fallback", value)
	}
	if value := source.GetClarity(t.Context(), w.Site); value.Reason != "snapshot_pending" || value.Metrics != nil {
		t.Fatal("no saved Clarity fallback", value)
	}
	value, err := jobs.Get(w.ID, w.Site, currentSnapshotQuarter())
	if err != nil || value.Status != "idle" {
		t.Fatal("GET enrolled background work", value, err)
	}
}
