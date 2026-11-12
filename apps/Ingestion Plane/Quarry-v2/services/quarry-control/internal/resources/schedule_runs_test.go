package resources

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
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

// decodeRegisterResult reads the registration answer. Like every control route it
// is wrapped in the {request_id, data} envelope.
func decodeRegisterResult(t *testing.T, body []byte) registerRunResult {
	t.Helper()
	var env struct {
		Data registerRunResult `json:"data"`
	}
	if err := json.Unmarshal(body, &env); err != nil {
		t.Fatalf("decode registration envelope: %v; body=%s", err, body)
	}
	return env.Data
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
	a := decodeRegisterResult(t, first.Body.Bytes())
	b := decodeRegisterResult(t, second.Body.Bytes())
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
	res := decodeRegisterResult(t, w.Body.Bytes())
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
	res := decodeRegisterResult(t, w.Body.Bytes())
	job, ok := db.Jobs().GetByOrg("org_a", quarrycontracts.ID(res.JobID))
	if !ok {
		t.Fatal("skipped job not found")
	}
	if job.Status != "skipped" || job.TerminalReason != "preflight_denied" || job.FinishedAt == nil {
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

// skipNoticeCapture is a notify.Sink that keeps the schedule notices it is asked to
// send. Only NotifyScheduleRun matters to this test.
type skipNoticeCapture struct {
	noticeTypes []string
	payloads    []map[string]string
}

func (s *skipNoticeCapture) NotifyChange(context.Context, string, string, string, map[string]any) error {
	return nil
}

func (s *skipNoticeCapture) NotifyScheduleRun(_ context.Context, _, _, _, noticeType string, _ bool, payload map[string]string) error {
	s.noticeTypes = append(s.noticeTypes, noticeType)
	s.payloads = append(s.payloads, payload)
	return nil
}

// A skipped fire's notice carries the fire's closed skip code as its reason, so the
// edge's specific reasons and the runtime sync's skipped_overlap reach the creator. A
// reason outside the closed vocabulary, or none, is the preflight_denied fallback.
func TestSkippedFireNoticeCarriesClosedSkipReason(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	sink := &skipNoticeCapture{}
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSchedules(r, db, WithScheduleNotices(sink))
	s := seedExecutableSchedule(t, db, "batch", true)

	cases := []struct{ key, reason, want string }{
		{"skip_overlap", "skipped_overlap", "skipped_overlap"},
		{"skip_zdr", "skipped_zdr", "skipped_zdr"},
		{"skip_posture", "skipped_posture_unknown", "skipped_posture_unknown"},
		{"skip_entitlement", "skipped_no_entitlement", "skipped_no_entitlement"},
		{"skip_kind", "skipped_kind_disabled", "skipped_kind_disabled"},
		{"skip_disabled", "skipped_disabled", "skipped_disabled"},
		{"preflight", "preflight_denied", "preflight_denied"},
		{"no_reason", "", "preflight_denied"},
		{"raw_reason", "https://example.com/raw", "preflight_denied"},
	}
	for i, tc := range cases {
		body, err := json.Marshal(map[string]string{
			"run_key": tc.key,
			"run_id":  "run_" + tc.key,
			"status":  "skipped",
			"reason":  tc.reason,
		})
		if err != nil {
			t.Fatalf("encode body: %v", err)
		}
		if w := postScheduleRun(r, s.ID, string(body)); w.Code != http.StatusCreated {
			t.Fatalf("reason %q: registration = %d; body=%s", tc.reason, w.Code, w.Body.String())
		}
		if len(sink.payloads) != i+1 {
			t.Fatalf("reason %q: %d schedule notices sent, want %d", tc.reason, len(sink.payloads), i+1)
		}
		if sink.noticeTypes[i] != notify.ScheduleRunSkippedType {
			t.Fatalf("reason %q: notice type = %q, want %q", tc.reason, sink.noticeTypes[i], notify.ScheduleRunSkippedType)
		}
		if got := sink.payloads[i]["reason"]; got != tc.want {
			t.Fatalf("reason %q: notice reason = %q, want %q", tc.reason, got, tc.want)
		}
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

// The change monitor's preflight switch is default off (WS15.S4a). Off, the args are
// the bytes that shipped, with no schedule id. On, the schedule id rides in, so that
// ChangeMonitorWF asks the edge before each fire.
func TestWorkflowFor_MonitorPreflightSwitchIsDefaultOff(t *testing.T) {
	s := store.Schedule{
		ID:         quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:      "org_a",
		CreatedBy:  "user_1",
		TargetKind: store.TargetKindChangeMonitor,
		TargetRef:  "https://example.com/p",
	}
	for _, v := range []string{"", "0", "off"} {
		t.Setenv(EnvMonitorPreflight, v)
		wf, args := workflowFor(s)
		raw, err := json.Marshal(args)
		if err != nil {
			t.Fatal(err)
		}
		if wf != "ChangeMonitorWF" || string(raw) != `[{"org_id":"org_a","url":"https://example.com/p","created_by":"user_1"}]` {
			t.Fatalf("switch %q: wf=%q args=%s, want the shipped args with no schedule_id", v, wf, raw)
		}
	}
	t.Setenv(EnvMonitorPreflight, "1")
	wf, args := workflowFor(s)
	a, ok := args[0].(changeMonitorArgs)
	if wf != "ChangeMonitorWF" || !ok || a.ScheduleID != string(s.ID) {
		t.Fatalf("switch on: wf=%q args=%v, want schedule_id %q", wf, args, s.ID)
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
