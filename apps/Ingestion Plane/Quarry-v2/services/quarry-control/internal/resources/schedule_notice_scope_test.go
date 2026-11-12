package resources

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// scheduleNoticeRecorder is a notify.Sink that keeps the schedule notices it is
// asked to send. The schedule notice path never calls NotifyChange.
type scheduleNoticeRecorder struct {
	mu      sync.Mutex
	notices []recordedNotice
}

type recordedNotice struct {
	orgID       string
	recipientID string
	noticeType  string
	payload     map[string]string
}

func (s *scheduleNoticeRecorder) NotifyChange(context.Context, string, string, string, map[string]any) error {
	return nil
}

func (s *scheduleNoticeRecorder) NotifyScheduleRun(_ context.Context, orgID, recipientID, _, noticeType string, _ bool, payload map[string]string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.notices = append(s.notices, recordedNotice{orgID: orgID, recipientID: recipientID, noticeType: noticeType, payload: payload})
	return nil
}

func (s *scheduleNoticeRecorder) sent() []recordedNotice {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]recordedNotice(nil), s.notices...)
}

// seedRunOwner stores an ordinary, unscheduled job of orgID that owns runID.
func seedRunOwner(t *testing.T, db store.DB, orgID string, runID quarrycontracts.ID) {
	t.Helper()
	rid := runID
	job := store.Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:     orgID,
		Kind:      "crawl",
		Status:    "running",
		Policy:    quarrycontracts.DefaultRunPolicy(),
		CreatedAt: 1,
		RunID:     &rid,
		Origin:    store.JobOrigin{Type: "api"},
	}
	if err := db.Jobs().Create(job); err != nil {
		t.Fatalf("seed run owner: %v", err)
	}
}

// seedScheduledFire stores a schedule of orgID created by userID, and the job of one
// of its fires, which owns runID.
func seedScheduledFire(t *testing.T, db store.DB, orgID, userID string, runID quarrycontracts.ID) (store.Schedule, store.Job) {
	t.Helper()
	sched := store.Schedule{
		ID:            quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:         orgID,
		Cron:          "* * * * *",
		TargetKind:    "crawl",
		TargetRef:     "https://example.com",
		Enabled:       true,
		CreatedAt:     1,
		CreatedBy:     userID,
		Name:          "nightly",
		Config:        map[string]any{"url": "https://example.com", "max_pages": 5},
		Timezone:      "UTC",
		OverlapPolicy: "skip",
		UpdatedAt:     1,
	}
	if err := db.Schedules().Create(sched); err != nil {
		t.Fatalf("seed schedule: %v", err)
	}
	sid := sched.ID
	rid := runID
	job := store.Job{
		ID:         quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:      orgID,
		Kind:       "crawl",
		Status:     "running",
		Policy:     quarrycontracts.DefaultRunPolicy(),
		ScheduleID: &sid,
		CreatedAt:  1,
		RunID:      &rid,
		Origin:     store.JobOrigin{Type: "schedule", Actor: string(sid), CreatedBy: userID},
	}
	if err := db.Jobs().Create(job); err != nil {
		t.Fatalf("seed scheduled job: %v", err)
	}
	return sched, job
}

// postRunFailed posts one run_failed event naming jobID to runID's events route, with
// the schedule notice sink mounted, and returns the status.
func postRunFailed(t *testing.T, db store.DB, sink *scheduleNoticeRecorder, runID, jobID quarrycontracts.ID) int {
	t.Helper()
	batch := []map[string]any{{
		"type":    string(quarrycontracts.EvtRunFailed),
		"job_id":  string(jobID),
		"payload": map[string]any{"reason": "child_failed"},
	}}
	body, err := json.Marshal(batch)
	if err != nil {
		t.Fatalf("encode batch: %v", err)
	}
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountEvents(r, db, "", nil, WithScheduleNotices(sink))
	req := httptest.NewRequest(http.MethodPost, "/v1/runs/"+string(runID)+"/events", strings.NewReader(string(body)))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)
	return w.Code
}

// A run_failed event posted to org A's run that names org B's scheduled job by job_id
// must not notify org B's schedule creator, since the notice would carry org B's
// schedule and run ids. The batch is still accepted.
func TestScheduleNotice_JobIDOfAnotherOrgSendsNothing(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	runA := quarrycontracts.NewID(quarrycontracts.KindRun)
	seedRunOwner(t, db, "org_a", runA)
	_, victim := seedScheduledFire(t, db, "org_b", "user_b", quarrycontracts.NewID(quarrycontracts.KindRun))
	sink := &scheduleNoticeRecorder{}
	if code := postRunFailed(t, db, sink, runA, victim.ID); code != http.StatusCreated {
		t.Fatalf("status = %d, want 201", code)
	}
	if got := sink.sent(); len(got) != 0 {
		t.Fatalf("cross-org job_id sent %d schedule notice(s), want none: %+v", len(got), got)
	}
}

// A run_failed event posted to a run that no job owns has no owning org, so it
// notifies nobody, even when its job_id names a scheduled job.
func TestScheduleNotice_UnknownRunSendsNothing(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	_, fire := seedScheduledFire(t, db, "org_b", "user_b", quarrycontracts.NewID(quarrycontracts.KindRun))
	sink := &scheduleNoticeRecorder{}
	if code := postRunFailed(t, db, sink, quarrycontracts.NewID(quarrycontracts.KindRun), fire.ID); code != http.StatusCreated {
		t.Fatalf("status = %d, want 201", code)
	}
	if got := sink.sent(); len(got) != 0 {
		t.Fatalf("unknown run sent %d schedule notice(s), want none: %+v", len(got), got)
	}
}

// A run_failed event for a scheduled fire of the run's own org still notifies the
// schedule's creator once, so the owner check only removes cross-tenant notices.
func TestScheduleNotice_SameOrgFireNotifiesCreator(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	runA := quarrycontracts.NewID(quarrycontracts.KindRun)
	sched, fire := seedScheduledFire(t, db, "org_a", "user_a", runA)
	sink := &scheduleNoticeRecorder{}
	if code := postRunFailed(t, db, sink, runA, fire.ID); code != http.StatusCreated {
		t.Fatalf("status = %d, want 201", code)
	}
	got := sink.sent()
	if len(got) != 1 {
		t.Fatalf("same-org fire sent %d schedule notice(s), want 1", len(got))
	}
	n := got[0]
	if n.orgID != "org_a" || n.recipientID != "user_a" || n.noticeType != notify.ScheduleRunFailedType {
		t.Fatalf("notice = %+v, want %s from org_a to user_a", n, notify.ScheduleRunFailedType)
	}
	if n.payload["schedule_id"] != string(sched.ID) || n.payload["run_id"] != string(runA) {
		t.Fatalf("notice payload = %+v, want schedule %s and run %s", n.payload, sched.ID, runA)
	}
}
