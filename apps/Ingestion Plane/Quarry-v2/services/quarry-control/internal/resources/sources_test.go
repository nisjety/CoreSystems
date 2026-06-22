package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func newSourcesServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSources(r, db)
	return r, db
}

// createSource POSTs a source for `org` and returns the minted source_id.
func createSource(t *testing.T, h http.Handler, org, body string) (int, string) {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, "/v1/sources?org_id="+org, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusCreated {
		return w.Code, ""
	}
	// httpx.WriteJSON wraps payloads in the REST envelope {data, meta},
	// so the minted source_id is nested under `data`.
	var env struct {
		Data struct {
			ID string `json:"source_id"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode created source: %v; body=%s", err, w.Body.String())
	}
	return w.Code, env.Data.ID
}

func listSourceIDs(t *testing.T, h http.Handler, org string) []string {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, "/v1/sources?org_id="+org, nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("list status=%d body=%s", w.Code, w.Body.String())
	}
	var env struct {
		Data struct {
			Items []struct {
				ID string `json:"source_id"`
			} `json:"items"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode list: %v; body=%s", err, w.Body.String())
	}
	ids := make([]string, 0, len(env.Data.Items))
	for _, it := range env.Data.Items {
		ids = append(ids, it.ID)
	}
	return ids
}

func TestSources_RequireOrgID(t *testing.T) {
	t.Parallel()
	h, _ := newSourcesServer(t)

	// GET / POST / DELETE all reject a missing org_id (no unscoped access).
	for _, tc := range []struct {
		method, path, body string
	}{
		{http.MethodGet, "/v1/sources", ""},
		{http.MethodPost, "/v1/sources", `{"name":"x","url":"https://x/","kind":"scrape"}`},
		{http.MethodDelete, "/v1/sources/src_01ABC", ""},
	} {
		var req *http.Request
		if tc.body != "" {
			req = httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
			req.Header.Set("Content-Type", "application/json")
		} else {
			req = httptest.NewRequest(tc.method, tc.path, nil)
		}
		w := httptest.NewRecorder()
		h.ServeHTTP(w, req)
		if w.Code != http.StatusBadRequest {
			t.Fatalf("%s %s without org_id: status=%d want=400; body=%s", tc.method, tc.path, w.Code, w.Body.String())
		}
	}
}

func TestSources_CreateListDelete_OrgScoped(t *testing.T) {
	t.Parallel()
	h, _ := newSourcesServer(t)

	code, aID := createSource(t, h, "org_a", `{"name":"A blog","url":"https://a.example/blog","kind":"scrape"}`)
	if code != http.StatusCreated || !strings.HasPrefix(aID, "src_") {
		t.Fatalf("create org_a: code=%d id=%q", code, aID)
	}
	if _, bID := createSource(t, h, "org_b", `{"name":"B blog","url":"https://b.example/blog","kind":"crawl"}`); bID == "" {
		t.Fatal("create org_b failed")
	}

	// Each org sees only its own source.
	if ids := listSourceIDs(t, h, "org_a"); len(ids) != 1 || ids[0] != aID {
		t.Fatalf("org_a list=%v want=[%s]", ids, aID)
	}
	if ids := listSourceIDs(t, h, "org_b"); len(ids) != 1 {
		t.Fatalf("org_b list=%v want len 1", ids)
	}

	// IDOR: org_b deleting org_a's source returns 404 and leaves it intact.
	del := func(org, id string) int {
		req := httptest.NewRequest(http.MethodDelete, "/v1/sources/"+id+"?org_id="+org, nil)
		w := httptest.NewRecorder()
		h.ServeHTTP(w, req)
		return w.Code
	}
	if code := del("org_b", aID); code != http.StatusNotFound {
		t.Fatalf("cross-tenant delete: code=%d want=404", code)
	}
	if ids := listSourceIDs(t, h, "org_a"); len(ids) != 1 {
		t.Fatalf("org_a source removed by cross-tenant delete: %v", ids)
	}

	// Owner delete → 204, then gone from list.
	if code := del("org_a", aID); code != http.StatusNoContent {
		t.Fatalf("owner delete: code=%d want=204", code)
	}
	if ids := listSourceIDs(t, h, "org_a"); len(ids) != 0 {
		t.Fatalf("org_a source still listed after delete: %v", ids)
	}
}

func TestSources_CreateValidation(t *testing.T) {
	t.Parallel()
	h, _ := newSourcesServer(t)

	// Missing name/url and bad kind are 400.
	for _, body := range []string{
		`{"url":"https://x/","kind":"scrape"}`,        // no name
		`{"name":"x","kind":"scrape"}`,                // no url
		`{"name":"x","url":"https://x/","kind":"foo"}`, // bad kind
	} {
		if code, _ := createSource(t, h, "org_a", body); code != http.StatusBadRequest {
			t.Fatalf("expected 400 for %q, got %d", body, code)
		}
	}
}

// TestSources_MonitorRegistersChangeSchedule proves the monitor flag also
// creates a change_monitor schedule (org-stamped) so the orchestrator
// materializes a Temporal schedule — the source→schedule wiring for PR-7.
func TestSources_MonitorRegistersChangeSchedule(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSources(r, db)

	code, id := createSource(t, r, "org_a",
		`{"name":"watch","url":"https://a.example/pricing","kind":"scrape","monitor":true,"preset":"daily"}`)
	if code != http.StatusCreated || id == "" {
		t.Fatalf("monitored create: code=%d id=%q", code, id)
	}

	scheds, _ := db.Schedules().List(50, "")
	if len(scheds) != 1 {
		t.Fatalf("expected exactly 1 change_monitor schedule, got %d", len(scheds))
	}
	s := scheds[0]
	if s.TargetKind != store.TargetKindChangeMonitor {
		t.Fatalf("schedule target_kind=%q want=change_monitor", s.TargetKind)
	}
	if s.OrgID != "org_a" {
		t.Fatalf("schedule org_id=%q want=org_a (stamped from verified query, not body)", s.OrgID)
	}
	if s.TargetRef != "https://a.example/pricing" {
		t.Fatalf("schedule target_ref=%q want the source url", s.TargetRef)
	}
	// daily preset resolves to a literal 5-field cron.
	if want, _ := store.PresetCron("daily"); s.Cron != want {
		t.Fatalf("schedule cron=%q want=%q", s.Cron, want)
	}
}
