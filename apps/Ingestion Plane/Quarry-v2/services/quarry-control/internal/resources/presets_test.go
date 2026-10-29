package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func newPresetServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountPresets(r)
	MountJobs(r, db)
	return r, db
}

func TestPresets_ListReturnsCatalog(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	var env map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	data, _ := env["data"].(map[string]any)
	items, _ := data["items"].([]any)
	if len(items) < 4 {
		t.Fatalf("expected 4+ presets in catalog, got %d", len(items))
	}
	names := map[string]bool{}
	for _, it := range items {
		m, _ := it.(map[string]any)
		n, _ := m["name"].(string)
		names[n] = true
	}
	for _, want := range []string{"fast", "polite", "stealth", "deterministic"} {
		if !names[want] {
			t.Errorf("preset %q missing from catalog", want)
		}
	}
}

func TestPresets_GetByName(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets/stealth", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	var env map[string]any
	_ = json.Unmarshal(w.Body.Bytes(), &env)
	data, _ := env["data"].(map[string]any)
	if data["name"] != "stealth" {
		t.Errorf("expected name=stealth, got %v", data["name"])
	}
	policy, _ := data["policy"].(map[string]any)
	if policy == nil {
		t.Fatal("policy missing from preset response")
	}
}

func TestPresets_GetUnknownReturns404(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	req := httptest.NewRequest(http.MethodGet, "/v1/presets/does-not-exist", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", w.Code)
	}
}

func TestCreateJob_AppliesPresetPolicy(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	body := `{"kind":"scrape","preset":"stealth","params":{"url":"https://x"}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	var env map[string]any
	_ = json.Unmarshal(w.Body.Bytes(), &env)
	data, _ := env["data"].(map[string]any)
	policy, _ := data["policy"].(map[string]any)
	concurrency, _ := policy["concurrency"].(map[string]any)
	if concurrency["per_run"].(float64) != 4 {
		t.Errorf("expected stealth per_run=4, got %v", concurrency["per_run"])
	}
}

func TestCreateJob_RejectsUnknownPreset(t *testing.T) {
	t.Parallel()
	h, _ := newPresetServer(t)
	body := `{"kind":"scrape","preset":"nosuch","params":{"url":"https://x"}}`
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs?org_id=org_test", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400; body=%s", w.Code, w.Body.String())
	}
}

func TestResolvePreset_DirectLookup(t *testing.T) {
	t.Parallel()
	p, ok := ResolvePreset("polite")
	if !ok {
		t.Fatal("polite preset not found")
	}
	def := quarrycontracts.DefaultRunPolicy()
	if p.Concurrency.PerRun != def.Concurrency.PerRun {
		t.Errorf("polite preset diverges from default policy: %+v vs %+v", p, def)
	}
}
