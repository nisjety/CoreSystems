package schedules

import (
	"context"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"
)

// A paused (disabled) schedule's overlap skips are not posted as skipped runs: control
// refuses a row for a disabled schedule with 409, so the sync holds them until the
// schedule is enabled again, then records them.
func TestSyncRuntime_PausedScheduleGetsNoSkipRowsUntilEnabled(t *testing.T) {
	t.Parallel()

	var runPosts atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPost && r.URL.Path == "/v1/internal/schedules/sch_1/runs" {
			runPosts.Add(1)
			w.WriteHeader(http.StatusCreated)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	m := New(nil, Config{ControlBaseURL: srv.URL, HTTPClient: srv.Client()})
	// Described a moment ago, with two overlap skips no row covers yet: a pass reaches
	// only the skip-row step and makes no Temporal describe call.
	m.runtime = map[string]*runtimeTrack{
		"sch_1": {describedAt: time.Now(), seeded: true, rowsPushed: 0, skips: 2},
	}
	existing := map[string]existingSchedule{"sch_1": {OrgID: "org_a"}}
	desiredOrgs := map[string]struct{}{"org_a": {}}
	spec := ScheduleSpec{ID: "sch_1", OrgID: "org_a", Workflow: ScheduledJobWorkflow, Paused: true}
	ctx := context.Background()

	m.syncRuntime(ctx, &fakeScheduleClient{}, existing, map[string]ScheduleSpec{"sch_1": spec}, desiredOrgs)
	if got := runPosts.Load(); got != 0 {
		t.Fatalf("paused schedule: %d skipped-run rows posted, want 0", got)
	}
	if got := m.runtime["sch_1"].rowsPushed; got != 0 {
		t.Fatalf("paused schedule: rowsPushed=%d, want 0 (backlog kept)", got)
	}

	spec.Paused = false
	m.syncRuntime(ctx, &fakeScheduleClient{}, existing, map[string]ScheduleSpec{"sch_1": spec}, desiredOrgs)
	if got := runPosts.Load(); got != 2 {
		t.Fatalf("enabled schedule: %d skipped-run rows posted, want 2", got)
	}
	if got := m.runtime["sch_1"].rowsPushed; got != 2 {
		t.Fatalf("enabled schedule: rowsPushed=%d, want 2", got)
	}
}
