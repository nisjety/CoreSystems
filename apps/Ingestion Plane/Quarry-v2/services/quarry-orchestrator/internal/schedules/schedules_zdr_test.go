package schedules

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"go.temporal.io/sdk/client"
)

// WS4.H: the ZDR stamp rides in quarry-control's schedule args and the
// reconciler passes args through as generic JSON. These tests pin that the
// stamp survives the whole path (control list payload -> fetchDesired ->
// reconcileWith -> the Temporal action args) for BOTH a first create and an
// update of an existing schedule, so a typed-args refactor that dropped the
// field would fail here.

const zdrControlList = `[` +
	`{"id":"sch_z","name":"z","org_id":"org_z","cron":"0 6 * * *","workflow":"ChangeMonitorWF",` +
	`"args":[{"org_id":"org_z","url":"https://example.com/p","created_by":"u","zdr":true}],"paused":false},` +
	`{"id":"sch_s","name":"s","org_id":"org_s","cron":"0 6 * * *","workflow":"ChangeMonitorWF",` +
	`"args":[{"org_id":"org_s","url":"https://example.com/q","created_by":"u","zdr":false}],"paused":false},` +
	`{"id":"sch_l","name":"l","org_id":"org_l","cron":"0 6 * * *","workflow":"ChangeMonitorWF",` +
	`"args":[{"org_id":"org_l","url":"https://example.com/r","created_by":"u"}],"paused":false}]`

func desiredFromControl(t *testing.T) []ScheduleSpec {
	t.Helper()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(zdrControlList))
	}))
	t.Cleanup(srv.Close)
	m := New(nil, Config{ControlBaseURL: srv.URL, HTTPClient: srv.Client()})
	got, err := m.fetchDesired(context.Background())
	if err != nil {
		t.Fatalf("fetchDesired: %v", err)
	}
	if len(got) != 3 {
		t.Fatalf("desired len=%d want=3", len(got))
	}
	return got
}

// firstArg returns args[0] of a Temporal workflow action as the generic map the
// reconciler hands to the SDK.
func firstArg(t *testing.T, action client.ScheduleAction) map[string]interface{} {
	t.Helper()
	wa, ok := action.(*client.ScheduleWorkflowAction)
	if !ok {
		t.Fatalf("action type=%T want=*client.ScheduleWorkflowAction", action)
	}
	if len(wa.Args) != 1 {
		t.Fatalf("args len=%d want=1: %v", len(wa.Args), wa.Args)
	}
	m, ok := wa.Args[0].(map[string]interface{})
	if !ok {
		t.Fatalf("args[0] type=%T want a generic map", wa.Args[0])
	}
	return m
}

// wantStamp asserts the zdr key of a decoded args map: nil = absent, else the
// exact stamped value.
func wantStamp(t *testing.T, id string, args map[string]interface{}, want *bool) {
	t.Helper()
	v, present := args["zdr"]
	switch {
	case want == nil && present:
		t.Fatalf("%s: an unstamped schedule must carry no zdr key, got %v", id, v)
	case want != nil && !present:
		t.Fatalf("%s: the stamp %v was dropped between control's list and the Temporal args: %v", id, *want, args)
	case want != nil && v != *want:
		t.Fatalf("%s: zdr=%v want %v", id, v, *want)
	}
}

func TestReconcileCreateCarriesTheZdrStampIntoTheTemporalArgs(t *testing.T) {
	t.Parallel()
	yes, no := true, false
	want := map[string]*bool{"sch_z": &yes, "sch_s": &no, "sch_l": nil}

	m := New(nil, Config{TaskQueue: "tasks"})
	sc := &fakeScheduleClient{}
	if err := m.reconcileWith(context.Background(), sc, desiredFromControl(t)); err != nil {
		t.Fatalf("reconcileWith: %v", err)
	}
	if len(sc.createOpts) != 3 {
		t.Fatalf("created %d schedules, want 3", len(sc.createOpts))
	}
	for _, opts := range sc.createOpts {
		wantStamp(t, opts.ID, firstArg(t, opts.Action), want[opts.ID])
	}
}

func TestReconcileUpdateCarriesTheZdrStampIntoTheTemporalArgs(t *testing.T) {
	t.Parallel()
	yes := true
	// The schedule already exists in Temporal (created before the stamp, or
	// before the row was re-stamped): the reconciler's update must push the
	// control row's args, stamp included, so a changed row reaches Temporal on
	// the next pass.
	h := &fakeScheduleHandle{id: "sch_z"}
	sc := &fakeScheduleClient{handle: h, listIter: &fakeScheduleIterator{entries: []*client.ScheduleListEntry{{ID: "sch_z"}}}}
	m := New(nil, Config{TaskQueue: "tasks"})

	var only []ScheduleSpec
	for _, s := range desiredFromControl(t) {
		if s.ID == "sch_z" {
			only = append(only, s)
		}
	}
	if err := m.reconcileWith(context.Background(), sc, only); err != nil {
		t.Fatalf("reconcileWith: %v", err)
	}
	if len(sc.createOpts) != 0 {
		t.Fatalf("an existing schedule must be updated, not created: %d creates", len(sc.createOpts))
	}
	if !h.updated || h.updatedWith == nil || h.updatedWith.Schedule == nil {
		t.Fatalf("expected an update with a schedule, got updated=%v with=%v", h.updated, h.updatedWith)
	}
	wantStamp(t, "sch_z", firstArg(t, h.updatedWith.Schedule.Action), &yes)
}
