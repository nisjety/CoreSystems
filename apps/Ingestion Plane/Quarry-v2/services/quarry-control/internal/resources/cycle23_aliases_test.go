// F3 follow-up tests: schedule trigger/backfill return 501 with a typed
// UNSUPPORTED envelope when the Temporal client isn't wired, instead
// of the previous 202-fake-accepted lie.
package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// scheduleTestServer mirrors the other `new*Server` helpers in the
// package: an in-memory `store.NewMemory()` + a chi router with
// `MountScheduleAliases(r, db, nil)` so the route lands but no
// Temporal client is wired. The third arg being `nil` is the test
// condition — every call here is the "Temporal not configured"
// path.
func scheduleAliasTestServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	r := chi.NewRouter()
	db := store.NewMemory()
	MountScheduleAliases(r, db, nil)
	return r, db
}

func decodeErrorBody(t *testing.T, w *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var env map[string]any
	if err := json.NewDecoder(w.Body).Decode(&env); err != nil {
		t.Fatalf("decode error body: %v; raw=%s", err, w.Body.String())
	}
	return env
}

func TestScheduleTrigger_NilClient_Returns501(t *testing.T) {
	h, db := scheduleAliasTestServer(t)
	// Seed a schedule so the lookup finds it (the 404 path is
	// covered separately).
	id := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	if err := db.Schedules().Create(store.Schedule{
		ID: id, OrgID: "org_a", Cron: "*/5 * * * *",
		TargetKind: "scrape",
		TargetRef:  "https://example.com",
		Enabled:    true,
	}); err != nil {
		t.Fatalf("seed schedule: %v", err)
	}

	req := httptest.NewRequest(http.MethodPost, "/v1/schedules/"+string(id)+"/trigger", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusNotImplemented {
		t.Fatalf("expected 501, got %d (body=%s)", w.Code, w.Body.String())
	}
	body := decodeErrorBody(t, w)
	errEnv, ok := body["error"].(map[string]any)
	if !ok {
		t.Fatalf("expected envelope.error, got %v", body)
	}
	if errEnv["code"] != "UNSUPPORTED" {
		t.Fatalf("expected code=UNSUPPORTED, got %v", errEnv["code"])
	}
}

func TestScheduleBackfill_NilClient_Returns501(t *testing.T) {
	h, db := scheduleAliasTestServer(t)
	id := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	if err := db.Schedules().Create(store.Schedule{
		ID: id, OrgID: "org_a", Cron: "*/5 * * * *",
		TargetKind: "scrape",
		TargetRef:  "https://example.com",
		Enabled:    true,
	}); err != nil {
		t.Fatalf("seed schedule: %v", err)
	}

	body := `{"start_at":"2026-01-01T00:00:00Z","end_at":"2026-01-02T00:00:00Z","overlap_policy":"skip"}`
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules/"+string(id)+"/backfill", strings.NewReader(body))
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)

	if w.Code != http.StatusNotImplemented {
		t.Fatalf("expected 501, got %d (body=%s)", w.Code, w.Body.String())
	}
	env := decodeErrorBody(t, w)
	errEnv, ok := env["error"].(map[string]any)
	if !ok {
		t.Fatalf("expected envelope.error, got %v", env)
	}
	if errEnv["code"] != "UNSUPPORTED" {
		t.Fatalf("expected code=UNSUPPORTED, got %v", errEnv["code"])
	}
}

// TestScheduleTrigger_MissingSchedule_Returns404: when the schedule
// id is unknown, we 404 first regardless of whether Temporal is
// wired. Locks the lookup order: ID-existence before client check.
func TestScheduleTrigger_MissingSchedule_Returns404(t *testing.T) {
	h, _ := scheduleAliasTestServer(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/schedules/sch_does_not_exist/trigger", nil)
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("expected 404, got %d (body=%s)", w.Code, w.Body.String())
	}
}
