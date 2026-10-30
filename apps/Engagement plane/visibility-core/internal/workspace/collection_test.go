package workspace

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func TestCollectionInventoryNeverUsesUncachedProviderFallback(t *testing.T) {
	var calls atomic.Int64
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); w.WriteHeader(500) }))
	defer upstream.Close()
	registry := workspaceRegistry(t, nil)
	w, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	owner := New(pulse.Config{IntegrationCoreURL: upstream.URL}, registry)
	v, err := owner.CollectionInventory(t.Context(), w.ID, w.Site, currentSnapshotQuarter())
	if err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 0 || len(v.Datasets) != 22 || v.Summary.Available != 0 || v.Summary.Unsupported != 3 {
		t.Fatal("collection side effect or fabricated availability", calls.Load(), v)
	}
	for i, d := range v.Datasets {
		if d.Records != nil || d.LastSuccessAt != "" {
			t.Fatal("invented receipt", d)
		}
		if i > 0 && v.Datasets[i-1].Key >= d.Key {
			t.Fatal("unstable dataset ordering")
		}
	}
	raw, _ := json.Marshal(v)
	if len(raw) > 32768 {
		t.Fatal("unbounded inventory")
	}
	if _, err = owner.CollectionInventory(t.Context(), w.ID, "https://other.no/", currentSnapshotQuarter()); err == nil {
		t.Fatal("foreign site accepted")
	}
	if _, err = owner.CollectionInventory(t.Context(), w.ID, w.Site, "all-time"); err == nil {
		t.Fatal("arbitrary period accepted")
	}
}
