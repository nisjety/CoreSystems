package schedules

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"go.temporal.io/sdk/client"
)

func TestNew_Defaults(t *testing.T) {
	t.Parallel()

	m := New(nil, Config{})
	if m == nil {
		t.Fatal("expected manager")
	}
	if m.cfg.Interval != 30*time.Second {
		t.Fatalf("default interval=%s want=%s", m.cfg.Interval, 30*time.Second)
	}
	if m.cfg.HTTPClient == nil {
		t.Fatal("expected default http client")
	}
	if m.cfg.HTTPClient.Timeout != 30*time.Second {
		t.Fatalf("default client timeout=%s want=%s", m.cfg.HTTPClient.Timeout, 30*time.Second)
	}
}

func TestNew_UsesProvidedConfig(t *testing.T) {
	t.Parallel()

	client := &http.Client{Timeout: 5 * time.Second}
	m := New(nil, Config{Interval: 5 * time.Minute, HTTPClient: client})

	if m.cfg.Interval != 5*time.Minute {
		t.Fatalf("interval=%s want=%s", m.cfg.Interval, 5*time.Minute)
	}
	if m.cfg.HTTPClient != client {
		t.Fatal("expected provided http client to be reused")
	}
}

func TestFetchDesired(t *testing.T) {
	t.Parallel()

	t.Run("success decodes payload", func(t *testing.T) {
		t.Parallel()

		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.Method != http.MethodGet {
				t.Fatalf("method=%s want=%s", r.Method, http.MethodGet)
			}
			if r.URL.Path != "/v1/schedules" {
				t.Fatalf("path=%s want=/v1/schedules", r.URL.Path)
			}
			if got := r.Header.Get("Authorization"); got != "Bearer token" {
				t.Fatalf("authorization=%q want=%q", got, "Bearer token")
			}
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`[{"id":"s1","name":"N","cron":"*/5 * * * *","workflow":"wf","args":["x"],"paused":true}]`))
		}))
		defer srv.Close()

		m := New(nil, Config{
			ControlBaseURL:   srv.URL,
			ControlAuthToken: "token",
			HTTPClient:       srv.Client(),
		})

		got, err := m.fetchDesired(context.Background())
		if err != nil {
			t.Fatalf("fetchDesired error: %v", err)
		}
		if len(got) != 1 {
			t.Fatalf("len=%d want=1", len(got))
		}
		if got[0].ID != "s1" || !got[0].Paused {
			t.Fatalf("decoded schedule mismatch: %+v", got[0])
		}
	})

	t.Run("non-2xx returns error", func(t *testing.T) {
		t.Parallel()

		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			http.Error(w, "bad", http.StatusBadGateway)
		}))
		defer srv.Close()

		m := New(nil, Config{ControlBaseURL: srv.URL, HTTPClient: srv.Client()})
		_, err := m.fetchDesired(context.Background())
		if err == nil {
			t.Fatal("expected error")
		}
		if !strings.Contains(err.Error(), "502") {
			t.Fatalf("expected status in error, got=%v", err)
		}
	})

	t.Run("decode error", func(t *testing.T) {
		t.Parallel()

		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			_, _ = w.Write([]byte("not-json"))
		}))
		defer srv.Close()

		m := New(nil, Config{ControlBaseURL: srv.URL, HTTPClient: srv.Client()})
		_, err := m.fetchDesired(context.Background())
		if err == nil {
			t.Fatal("expected decode error")
		}
		if !strings.Contains(err.Error(), "decode") {
			t.Fatalf("expected decode error, got=%v", err)
		}
	})
}

type fakeScheduleIterator struct {
	entries []*client.ScheduleListEntry
	err     error
	idx     int
}

func (it *fakeScheduleIterator) HasNext() bool {
	return it.err == nil && it.idx < len(it.entries)
}
func (it *fakeScheduleIterator) Next() (*client.ScheduleListEntry, error) {
	if it.err != nil {
		return nil, it.err
	}
	if !it.HasNext() {
		return nil, errors.New("no more")
	}
	e := it.entries[it.idx]
	it.idx++
	return e, nil
}

type fakeScheduleHandle struct {
	id          string
	deleteErr   error
	updateErr   error
	pauseErr    error
	unpauseErr  error
	deleted     bool
	updated     bool
	paused      bool
	unpaused    bool
	updatedWith *client.ScheduleUpdate
}

func (h *fakeScheduleHandle) GetID() string { return h.id }
func (h *fakeScheduleHandle) Delete(context.Context) error {
	h.deleted = true
	return h.deleteErr
}
func (h *fakeScheduleHandle) Backfill(context.Context, client.ScheduleBackfillOptions) error {
	return nil
}
func (h *fakeScheduleHandle) Update(_ context.Context, opts client.ScheduleUpdateOptions) error {
	if h.updateErr != nil {
		return h.updateErr
	}
	if opts.DoUpdate != nil {
		out, err := opts.DoUpdate(client.ScheduleUpdateInput{Description: client.ScheduleDescription{Schedule: client.Schedule{}}})
		if err != nil {
			return err
		}
		h.updatedWith = out
	}
	h.updated = true
	return nil
}
func (h *fakeScheduleHandle) Describe(context.Context) (*client.ScheduleDescription, error) {
	return &client.ScheduleDescription{}, nil
}
func (h *fakeScheduleHandle) Trigger(context.Context, client.ScheduleTriggerOptions) error {
	return nil
}
func (h *fakeScheduleHandle) Pause(context.Context, client.SchedulePauseOptions) error {
	h.paused = true
	h.unpaused = false
	return h.pauseErr
}
func (h *fakeScheduleHandle) Unpause(context.Context, client.ScheduleUnpauseOptions) error {
	h.paused = false
	h.unpaused = true
	return h.unpauseErr
}

type fakeScheduleClient struct {
	handle     *fakeScheduleHandle
	listIter   client.ScheduleListIterator
	listErr    error
	createErr  error
	createOpts []client.ScheduleOptions
}

func (sc *fakeScheduleClient) Create(_ context.Context, opts client.ScheduleOptions) (client.ScheduleHandle, error) {
	sc.createOpts = append(sc.createOpts, opts)
	if sc.createErr != nil {
		return nil, sc.createErr
	}
	if sc.handle == nil {
		sc.handle = &fakeScheduleHandle{id: opts.ID}
	}
	return sc.handle, nil
}
func (sc *fakeScheduleClient) List(context.Context, client.ScheduleListOptions) (client.ScheduleListIterator, error) {
	if sc.listErr != nil {
		return nil, sc.listErr
	}
	if sc.listIter == nil {
		sc.listIter = &fakeScheduleIterator{}
	}
	return sc.listIter, nil
}
func (sc *fakeScheduleClient) GetHandle(_ context.Context, scheduleID string) client.ScheduleHandle {
	if sc.handle == nil {
		sc.handle = &fakeScheduleHandle{id: scheduleID}
	}
	return sc.handle
}

func TestReconcileAndRun_FetchErrors(t *testing.T) {
	t.Parallel()

	m := New(nil, Config{ControlBaseURL: "http://[::1"})
	if err := m.reconcile(context.Background()); err == nil || !strings.Contains(err.Error(), "fetch desired") {
		t.Fatalf("expected wrapped fetch desired error, got=%v", err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	m = New(nil, Config{ControlBaseURL: "http://127.0.0.1:1", Interval: time.Hour, HTTPClient: &http.Client{Timeout: 5 * time.Millisecond}})
	if err := m.Run(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("Run error=%v want context.Canceled", err)
	}
}

func TestCreate_BuildsTemporalScheduleOptions(t *testing.T) {
	t.Parallel()

	m := New(nil, Config{TaskQueue: "tasks"})
	sc := &fakeScheduleClient{}
	spec := ScheduleSpec{ID: "sch_1", Name: "name", Cron: "*/5 * * * *", Workflow: "wf", Args: []interface{}{"a"}, Paused: true}

	if err := m.create(context.Background(), sc, spec); err != nil {
		t.Fatalf("create: %v", err)
	}
	if len(sc.createOpts) != 1 {
		t.Fatalf("create opts len=%d want=1", len(sc.createOpts))
	}
	opts := sc.createOpts[0]
	if opts.ID != spec.ID || opts.Note != spec.Name || !opts.Paused {
		t.Fatalf("unexpected schedule options: %+v", opts)
	}
	if len(opts.Spec.CronExpressions) != 1 || opts.Spec.CronExpressions[0] != spec.Cron {
		t.Fatalf("unexpected cron spec: %+v", opts.Spec)
	}
	action, ok := opts.Action.(*client.ScheduleWorkflowAction)
	if !ok {
		t.Fatalf("action type=%T want=*client.ScheduleWorkflowAction", opts.Action)
	}
	if action.Workflow != spec.Workflow || action.TaskQueue != "tasks" {
		t.Fatalf("unexpected workflow action: %+v", action)
	}
	if len(action.Args) != len(spec.Args) {
		t.Fatalf("args len=%d want=%d", len(action.Args), len(spec.Args))
	}
	for i := range spec.Args {
		if action.Args[i] != spec.Args[i] {
			t.Fatalf("arg[%d]=%v want=%v", i, action.Args[i], spec.Args[i])
		}
	}
}

func TestUpdate_ReconcilesPauseAndUnpause(t *testing.T) {
	t.Parallel()

	t.Run("paused schedule pauses handle", func(t *testing.T) {
		t.Parallel()
		h := &fakeScheduleHandle{id: "sch_1"}
		sc := &fakeScheduleClient{handle: h}
		m := New(nil, Config{TaskQueue: "tasks"})

		err := m.update(context.Background(), sc, ScheduleSpec{ID: "sch_1", Cron: "*/5 * * * *", Workflow: "wf", Paused: true})
		if err != nil {
			t.Fatalf("update paused: %v", err)
		}
		if !h.updated || !h.paused || h.unpaused {
			t.Fatalf("expected updated+paused only, got updated=%v paused=%v unpaused=%v", h.updated, h.paused, h.unpaused)
		}
		if h.updatedWith == nil || h.updatedWith.Schedule == nil || h.updatedWith.Schedule.Spec == nil {
			t.Fatal("expected update callback to populate schedule")
		}
	})

	t.Run("unpaused schedule unpauses handle", func(t *testing.T) {
		t.Parallel()
		h := &fakeScheduleHandle{id: "sch_1"}
		sc := &fakeScheduleClient{handle: h}
		m := New(nil, Config{TaskQueue: "tasks"})

		err := m.update(context.Background(), sc, ScheduleSpec{ID: "sch_1", Cron: "*/5 * * * *", Workflow: "wf", Paused: false})
		if err != nil {
			t.Fatalf("update unpaused: %v", err)
		}
		if !h.updated || h.paused || !h.unpaused {
			t.Fatalf("expected updated+unpaused only, got updated=%v paused=%v unpaused=%v", h.updated, h.paused, h.unpaused)
		}
	})
}

func TestUpdate_WrapsErrors(t *testing.T) {
	t.Parallel()

	m := New(nil, Config{TaskQueue: "tasks"})

	t.Run("update error", func(t *testing.T) {
		t.Parallel()
		h := &fakeScheduleHandle{id: "sch_1", updateErr: errors.New("boom")}
		sc := &fakeScheduleClient{handle: h}
		err := m.update(context.Background(), sc, ScheduleSpec{ID: "sch_1", Cron: "* * * * *", Workflow: "wf"})
		if err == nil || !strings.Contains(err.Error(), "update:") {
			t.Fatalf("expected wrapped update error, got=%v", err)
		}
	})

	t.Run("pause error", func(t *testing.T) {
		t.Parallel()
		h := &fakeScheduleHandle{id: "sch_1", pauseErr: errors.New("pause boom")}
		sc := &fakeScheduleClient{handle: h}
		err := m.update(context.Background(), sc, ScheduleSpec{ID: "sch_1", Cron: "* * * * *", Workflow: "wf", Paused: true})
		if err == nil || !strings.Contains(err.Error(), "pause:") {
			t.Fatalf("expected wrapped pause error, got=%v", err)
		}
	})

	t.Run("unpause error", func(t *testing.T) {
		t.Parallel()
		h := &fakeScheduleHandle{id: "sch_1", unpauseErr: errors.New("unpause boom")}
		sc := &fakeScheduleClient{handle: h}
		err := m.update(context.Background(), sc, ScheduleSpec{ID: "sch_1", Cron: "* * * * *", Workflow: "wf", Paused: false})
		if err == nil || !strings.Contains(err.Error(), "unpause:") {
			t.Fatalf("expected wrapped unpause error, got=%v", err)
		}
	})
}
