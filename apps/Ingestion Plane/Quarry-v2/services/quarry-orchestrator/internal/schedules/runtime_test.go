package schedules

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"go.temporal.io/sdk/client"
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

// A schedule recreated under the same id starts its skip counter again. Its skips are new,
// so they are recorded whatever count the old schedule left, and each run key carries the
// new creation time: the old schedule's rows use the same skip numbers, so a bare number
// would collide with them. A schedule that is not recreated keeps its baseline.
func TestSyncRuntime_RecreatedScheduleRecordsItsOwnSkips(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		track   runtimeTrack
		skips   int
		created int64
		want    []string
	}{
		{
			name:    "count went back",
			track:   runtimeTrack{seeded: true, rowsPushed: 5, skips: 5, incarnation: 1000},
			skips:   1,
			created: 2000,
			want:    []string{"sched:sch_1:2000:skip:1"},
		},
		{
			name:    "count went past the old one",
			track:   runtimeTrack{seeded: true, rowsPushed: 2, skips: 2, incarnation: 1000},
			skips:   3,
			created: 2000,
			want:    []string{"sched:sch_1:2000:skip:1", "sched:sch_1:2000:skip:2", "sched:sch_1:2000:skip:3"},
		},
		{
			name:    "same schedule, one new skip",
			track:   runtimeTrack{seeded: true, rowsPushed: 2, skips: 2, incarnation: 1000},
			skips:   3,
			created: 1000,
			want:    []string{"sched:sch_1:1000:skip:3"},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			var (
				mu   sync.Mutex
				keys []string
			)
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Method == http.MethodPost && r.URL.Path == "/v1/internal/schedules/sch_1/runs" {
					var body skipRowBody
					_ = json.NewDecoder(r.Body).Decode(&body)
					mu.Lock()
					keys = append(keys, body.RunKey)
					mu.Unlock()
					w.WriteHeader(http.StatusCreated)
					return
				}
				w.WriteHeader(http.StatusNoContent)
			}))
			defer srv.Close()

			m := New(nil, Config{ControlBaseURL: srv.URL, HTTPClient: srv.Client()})
			track := tc.track
			track.describedAt = time.Now().Add(-time.Hour)
			m.runtime = map[string]*runtimeTrack{"sch_1": &track}
			sc := runtimeDescribeClient{skips: tc.skips, created: time.UnixMilli(tc.created)}
			existing := map[string]existingSchedule{"sch_1": {OrgID: "org_a"}}
			desired := map[string]ScheduleSpec{"sch_1": {ID: "sch_1", OrgID: "org_a", Workflow: ScheduledJobWorkflow}}
			m.syncRuntime(context.Background(), sc, existing, desired, map[string]struct{}{"org_a": {}})

			mu.Lock()
			defer mu.Unlock()
			if !reflect.DeepEqual(keys, tc.want) {
				t.Fatalf("skipped-run keys=%q, want %q", keys, tc.want)
			}
		})
	}
}

// runtimeDescribeClient is a schedule client whose every describe reports one skip count
// and one creation time. The runtime sync calls nothing else on it.
type runtimeDescribeClient struct {
	client.ScheduleClient
	skips   int
	created time.Time
}

func (c runtimeDescribeClient) GetHandle(context.Context, string) client.ScheduleHandle {
	return runtimeDescribeHandle{c: c}
}

type runtimeDescribeHandle struct {
	client.ScheduleHandle
	c runtimeDescribeClient
}

func (h runtimeDescribeHandle) Describe(context.Context) (*client.ScheduleDescription, error) {
	var desc client.ScheduleDescription
	desc.Info.NumActionsSkippedOverlap = h.c.skips
	desc.Info.CreatedAt = h.c.created
	return &desc, nil
}
