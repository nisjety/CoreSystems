package resources

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func admissionServer(t *testing.T, adm Admission) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountJobs(r, db, WithAdmission(adm))
	return r, db
}

func postJob(h http.Handler, org string, body map[string]any) *httptest.ResponseRecorder {
	encoded, _ := json.Marshal(body)
	req := httptest.NewRequest(http.MethodPost, "/v1/jobs/?org_id="+org, bytes.NewReader(encoded))
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w
}

func enabled() Admission {
	return Admission{Enabled: true, CapDefault: 2, BacklogDefault: 2, BacklogTTL: time.Hour, ClaimLease: time.Minute}
}

func TestBacklogFullReturns429WithDetailsAndStoresNothing(t *testing.T) {
	t.Parallel()
	h, db := admissionServer(t, enabled())
	for i := 0; i < 2; i++ {
		if w := postJob(h, "org_a", map[string]any{"kind": "crawl", "params": map[string]any{"url": "https://example.com"}}); w.Code != http.StatusCreated {
			t.Fatalf("submission %d: %d %s", i, w.Code, w.Body.String())
		}
	}
	w := postJob(h, "org_a", map[string]any{"kind": "crawl", "params": map[string]any{"url": "https://example.com"}})
	if w.Code != http.StatusTooManyRequests {
		t.Fatalf("status = %d: %s", w.Code, w.Body.String())
	}
	if w.Header().Get("Retry-After") == "" {
		t.Fatal("a refused submission must say when to retry")
	}
	var env struct {
		Error struct {
			Code    string         `json:"code"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatal(err)
	}
	if env.Error.Code != "RATE_LIMITED" || env.Error.Details["reason"] != "org_backlog_full" ||
		env.Error.Details["limit"] != float64(2) || env.Error.Details["queued"] != float64(2) {
		t.Fatalf("error = %#v", env.Error)
	}
	if n := db.Jobs().CountAccepted("org_a", time.Now()); n != 2 {
		t.Fatalf("the refused submission left a row: %d waiting", n)
	}
	// Another organization is unaffected.
	if w := postJob(h, "org_b", map[string]any{"kind": "crawl"}); w.Code != http.StatusCreated {
		t.Fatalf("org b: %d", w.Code)
	}
}

func TestTheJobsOwnBacklogCapOverridesTheDefault(t *testing.T) {
	t.Parallel()
	h, _ := admissionServer(t, enabled())
	stamped := map[string]any{"admission": map[string]any{"cap": 1, "backlog_cap": 1}}
	if w := postJob(h, "org_a", map[string]any{"kind": "crawl", "params": stamped}); w.Code != http.StatusCreated {
		t.Fatalf("first: %d", w.Code)
	}
	if w := postJob(h, "org_a", map[string]any{"kind": "crawl", "params": stamped}); w.Code != http.StatusTooManyRequests {
		t.Fatalf("second should exceed the stamped backlog of 1: %d", w.Code)
	}
}

func TestAdmissionOffChangesNothing(t *testing.T) {
	t.Parallel()
	h, _ := admissionServer(t, Admission{})
	for i := 0; i < 20; i++ {
		if w := postJob(h, "org_a", map[string]any{"kind": "crawl"}); w.Code != http.StatusCreated {
			t.Fatalf("submission %d: %d", i, w.Code)
		}
	}
	req := httptest.NewRequest(http.MethodPost, "/v1/internal/jobs/claim", bytes.NewReader([]byte(`{"limit":5}`)))
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	if w.Code != http.StatusNotFound {
		t.Fatalf("the claim endpoint must be absent while admission is off, got %d", w.Code)
	}
}

func TestOnlyExecutableKindsAreBoundedByTheBacklog(t *testing.T) {
	t.Parallel()
	h, _ := admissionServer(t, enabled())
	for i := 0; i < 5; i++ {
		if w := postJob(h, "org_a", map[string]any{"kind": "search"}); w.Code != http.StatusCreated {
			t.Fatalf("a search job is not dispatched, so it is not bounded: %d", w.Code)
		}
	}
}

func TestClaimEndpointLeasesJobsAndRespectsTheCap(t *testing.T) {
	t.Parallel()
	h, _ := admissionServer(t, enabled())
	for i := 0; i < 2; i++ {
		postJob(h, "org_a", map[string]any{"kind": "crawl"})
	}
	claim := func() []map[string]any {
		req := httptest.NewRequest(http.MethodPost, "/v1/internal/jobs/claim", bytes.NewReader([]byte(`{"limit":10}`)))
		w := httptest.NewRecorder()
		h.ServeHTTP(w, req)
		if w.Code != http.StatusOK {
			t.Fatalf("claim: %d %s", w.Code, w.Body.String())
		}
		var env struct {
			Data []map[string]any `json:"data"`
		}
		if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
			t.Fatal(err)
		}
		return env.Data
	}
	if first := claim(); len(first) != 2 {
		t.Fatalf("claimed %d, want 2 (cap default 2)", len(first))
	}
	if again := claim(); len(again) != 0 {
		t.Fatalf("the same jobs were claimed twice: %d", len(again))
	}
}

func TestBacklogExpiryFailsOldJobsAndClosesTheirEventStream(t *testing.T) {
	t.Parallel()
	_, db := admissionServer(t, enabled())
	now := time.Now()
	oldID := quarrycontracts.NewID(quarrycontracts.KindJob)
	freshID := quarrycontracts.NewID(quarrycontracts.KindJob)
	for id, created := range map[quarrycontracts.ID]time.Time{oldID: now.Add(-2 * time.Hour), freshID: now.Add(-time.Minute)} {
		if err := db.Jobs().Create(store.Job{ID: id, OrgID: "org_a", Kind: "crawl", Status: "accepted", CreatedAt: created.UnixMilli()}); err != nil {
			t.Fatal(err)
		}
	}
	if n := ExpireBacklog(db, enabled(), now, zerolog.Nop()); n != 1 {
		t.Fatalf("expired %d, want 1", n)
	}
	old, _ := db.Jobs().Get(oldID)
	if old.Status != "failed" || old.Params["failure_reason"] != "backlog_expired" {
		t.Fatalf("old job = %#v", old)
	}
	if fresh, _ := db.Jobs().Get(freshID); fresh.Status != "accepted" {
		t.Fatalf("the fresh job must keep waiting: %#v", fresh)
	}
	events := db.Events().ForJob(oldID, 0, 10)
	if len(events) != 1 || events[0].Type != quarrycontracts.EvtRunFailed || events[0].Payload["reason"] != "backlog_expired" {
		t.Fatalf("expected one terminal run_failed event, got %#v", events)
	}
	if got := db.Events().ForJob(freshID, 0, 10); len(got) != 0 {
		t.Fatalf("the fresh job got events: %#v", got)
	}
}

func TestAdmissionFromEnv(t *testing.T) {
	t.Parallel()
	get := func(vars map[string]string) func(string) string {
		return func(name string) string { return vars[name] }
	}
	def, err := AdmissionFromEnv(get(nil))
	if err != nil || def.Enabled || def.CapDefault != 3 || def.BacklogDefault != 200 || def.BacklogTTL != 24*time.Hour || def.ClaimLease != time.Minute {
		t.Fatalf("defaults = %#v, %v", def, err)
	}
	on, err := AdmissionFromEnv(get(map[string]string{
		"QUARRY_JOB_ADMISSION": "on", "QUARRY_JOB_CAP_DEFAULT": "1", "QUARRY_JOB_BACKLOG_DEFAULT": "2",
		"QUARRY_JOB_BACKLOG_TTL": "60s", "QUARRY_JOB_CLAIM_LEASE": "5s",
	}))
	if err != nil || !on.Enabled || on.CapDefault != 1 || on.BacklogDefault != 2 || on.BacklogTTL != time.Minute || on.ClaimLease != 5*time.Second {
		t.Fatalf("configured = %#v, %v", on, err)
	}
	for name, bad := range map[string]string{
		"QUARRY_JOB_ADMISSION": "maybe", "QUARRY_JOB_CAP_DEFAULT": "0", "QUARRY_JOB_BACKLOG_DEFAULT": "many",
		"QUARRY_JOB_BACKLOG_TTL": "-5m", "QUARRY_JOB_CLAIM_LEASE": "soon",
	} {
		if _, err := AdmissionFromEnv(get(map[string]string{name: bad})); err == nil {
			t.Errorf("%s=%q must be rejected", name, bad)
		}
	}
}
