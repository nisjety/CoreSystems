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

// seedExecutableSchedule stores a schedule row for the fire-registration tests.
func seedExecutableSchedule(t *testing.T, db store.DB, kind string, enabled bool) store.Schedule {
	t.Helper()
	s := store.Schedule{
		ID:            quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:         "org_a",
		Cron:          "* * * * *",
		TargetKind:    kind,
		TargetRef:     "https://example.com",
		Enabled:       enabled,
		CreatedAt:     1,
		CreatedBy:     "user_1",
		Name:          "nightly",
		Config:        map[string]any{"url": "https://example.com", "max_pages": 5},
		Timezone:      "UTC",
		OverlapPolicy: "skip",
		UpdatedAt:     1,
	}
	if err := db.Schedules().Create(s); err != nil {
		t.Fatalf("seed schedule: %v", err)
	}
	return s
}

func postScheduleRun(h http.Handler, id quarrycontracts.ID, body string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(http.MethodPost, "/v1/internal/schedules/"+string(id)+"/runs", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w
}

func newRunsServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSchedules(r, db)
	return r, db
}

func TestRegisterRun_IdempotentByRunKey(t *testing.T) {
	t.Parallel()
	h, db := newRunsServer(t)
	s := seedExecutableSchedule(t, db, "crawl", true)
	body := `{"run_key":"run_k1","run_id":"run_r1","status":"running","workflow_id":"wf-1"}`

	first := postScheduleRun(h, s.ID, body)
	if first.Code != http.StatusCreated {
		t.Fatalf("first registration = %d, want 201; body=%s", first.Code, first.Body.String())
	}
	second := postScheduleRun(h, s.ID, body)
	if second.Code != http.StatusOK {
		t.Fatalf("retry registration = %d, want 200; body=%s", second.Code, second.Body.String())
	}
	var a, b registerRunResult
	_ = json.Unmarshal(first.Body.Bytes(), &a)
	_ = json.Unmarshal(second.Body.Bytes(), &b)
	if a.JobID == "" || a.JobID != b.JobID {
		t.Fatalf("retry must return the same job: first=%q second=%q", a.JobID, b.JobID)
	}
	items, _ := db.Jobs().ListBySchedule(s.ID, 10, "")
	if len(items) != 1 {
		t.Fatalf("jobs for the schedule = %d, want 1", len(items))
	}
}

func TestRegisterRun_DerivesOrgAndParamsFromScheduleNotBody(t *testing.T) {
	t.Parallel()
	h, db := newRunsServer(t)
	s := seedExecutableSchedule(t, db, "crawl", true)
	// The body tries to choose another tenant and other params. Both are ignored.
	body := `{"run_key":"run_k2","run_id":"run_r2","status":"running","org_id":"org_evil","params":{"org_id":"org_evil","url":"https://evil.example"}}`

	w := postScheduleRun(h, s.ID, body)
	if w.Code != http.StatusCreated {
		t.Fatalf("registration = %d; body=%s", w.Code, w.Body.String())
	}
	var res registerRunResult
	_ = json.Unmarshal(w.Body.Bytes(), &res)
	job, ok := db.Jobs().GetByOrg("org_a", quarrycontracts.ID(res.JobID))
	if !ok {
		t.Fatal("job not found under the schedule's org")
	}
	if job.OrgID != "org_a" || job.Params["org_id"] != "org_a" {
		t.Fatalf("org must come from the schedule: org=%q params.org_id=%v", job.OrgID, job.Params["org_id"])
	}
	if job.Params["url"] != "https://example.com" {
		t.Fatalf("params must come from the schedule config, got url=%v", job.Params["url"])
	}
	if job.Origin.Type != "schedule" || job.Origin.Actor != string(s.ID) {
		t.Fatalf("origin = %+v, want schedule/%s", job.Origin, s.ID)
	}
	if job.ScheduleID == nil || *job.ScheduleID != s.ID {
		t.Fatalf("schedule id not stamped on the job")
	}
}

func TestRegisterRun_DisabledOrMissingSchedule409(t *testing.T) {
	t.Parallel()
	h, db := newRunsServer(t)
	disabled := seedExecutableSchedule(t, db, "crawl", false)
	body := `{"run_key":"run_k3","run_id":"run_r3","status":"running"}`

	if w := postScheduleRun(h, disabled.ID, body); w.Code != http.StatusConflict {
		t.Fatalf("disabled schedule = %d, want 409", w.Code)
	}
	missing := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	if w := postScheduleRun(h, missing, body); w.Code != http.StatusNotFound {
		t.Fatalf("missing schedule = %d, want 404", w.Code)
	}
}

func TestRegisterRun_SkippedRowIsTerminalWithEvent(t *testing.T) {
	t.Parallel()
	h, db := newRunsServer(t)
	s := seedExecutableSchedule(t, db, "batch", true)
	body := `{"run_key":"run_k4","run_id":"run_r4","status":"skipped","reason":"preflight_denied"}`

	w := postScheduleRun(h, s.ID, body)
	if w.Code != http.StatusCreated {
		t.Fatalf("skipped registration = %d; body=%s", w.Code, w.Body.String())
	}
	var res registerRunResult
	_ = json.Unmarshal(w.Body.Bytes(), &res)
	job, ok := db.Jobs().GetByOrg("org_a", quarrycontracts.ID(res.JobID))
	if !ok {
		t.Fatal("skipped job not found")
	}
	if job.Status != "skipped" || job.TerminalReason != "schedule_skipped" || job.FinishedAt == nil {
		t.Fatalf("skipped job must be terminal: status=%q reason=%q finished=%v", job.Status, job.TerminalReason, job.FinishedAt)
	}
	events := db.Events().ForJob(quarrycontracts.ID(res.JobID), 0, 10)
	found := false
	for _, e := range events {
		if e.Type == quarrycontracts.EvtScheduleSkipped && e.Payload["reason"] == "preflight_denied" {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected a schedule_skipped event with the reason, got %d events", len(events))
	}
}

func TestRegisterRun_RejectsBadStatusAndMissingKey(t *testing.T) {
	t.Parallel()
	h, db := newRunsServer(t)
	s := seedExecutableSchedule(t, db, "scrape", true)
	for _, body := range []string{
		`{"run_key":"","run_id":"run_r5","status":"running"}`,
		`{"run_key":"run_k5","run_id":"run_r5","status":"completed"}`,
	} {
		if w := postScheduleRun(h, s.ID, body); w.Code != http.StatusBadRequest {
			t.Fatalf("body %s = %d, want 400", body, w.Code)
		}
	}
}

func TestWorkflowFor_KillSwitchOffReturnsEmpty(t *testing.T) {
	t.Setenv(EnvScheduledJobsEnabled, "0")
	for _, kind := range []string{"scrape", "crawl", "batch"} {
		wf, args := workflowFor(store.Schedule{ID: quarrycontracts.NewID(quarrycontracts.KindSchedule), OrgID: "org_a", TargetKind: kind})
		if wf != "" || args != nil {
			t.Fatalf("%s with the switch off = (%q, %v), want empty", kind, wf, args)
		}
	}
}

func TestWorkflowFor_CrawlScrapeBatchBuildScheduledArgs(t *testing.T) {
	t.Setenv(EnvScheduledJobsEnabled, "1")
	for _, kind := range []string{"scrape", "crawl", "batch"} {
		s := store.Schedule{
			ID:         quarrycontracts.NewID(quarrycontracts.KindSchedule),
			OrgID:      "org_a",
			CreatedBy:  "user_1",
			TargetKind: kind,
			Config:     map[string]any{"url": "https://example.com"},
		}
		wf, args := workflowFor(s)
		if wf != "ScheduledJobWF" || len(args) != 1 {
			t.Fatalf("%s = (%q, %d args), want ScheduledJobWF with one arg", kind, wf, len(args))
		}
		a, ok := args[0].(scheduledJobArgs)
		if !ok {
			t.Fatalf("%s arg type = %T", kind, args[0])
		}
		if a.Kind != kind || a.OrgID != "org_a" || a.ScheduleID != string(s.ID) || a.Params["org_id"] != "org_a" {
			t.Fatalf("%s args = %+v", kind, a)
		}
	}
}
