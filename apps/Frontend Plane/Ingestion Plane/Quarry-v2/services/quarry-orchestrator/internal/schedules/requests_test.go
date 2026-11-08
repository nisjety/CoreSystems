package schedules

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/rs/zerolog"
	"go.temporal.io/api/serviceerror"
)

// fakeTemporal records the calls the loop makes and answers with errs in order.
type fakeTemporal struct {
	mu        sync.Mutex
	triggers  []string
	backfills []string
	windows   [][2]time.Time
	errs      []error
}

func (f *fakeTemporal) next() error {
	if len(f.errs) == 0 {
		return nil
	}
	err := f.errs[0]
	f.errs = f.errs[1:]
	return err
}

func (f *fakeTemporal) Trigger(_ context.Context, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.triggers = append(f.triggers, id)
	return f.next()
}

func (f *fakeTemporal) Backfill(_ context.Context, id string, start, end time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.backfills = append(f.backfills, id)
	f.windows = append(f.windows, [2]time.Time{start, end})
	return f.next()
}

// fakeControl serves the claim and result routes. Each claim hands out the next
// batch; every result post is recorded with its signature headers.
type fakeControl struct {
	mu      sync.Mutex
	batches [][]claimedRequest
	results map[string][]requestOutcome
	claims  []map[string]any
	signed  []*http.Request
	bodies  [][]byte
}

func newFakeControl(batches ...[]claimedRequest) *fakeControl {
	return &fakeControl{batches: batches, results: map[string][]requestOutcome{}}
}

func (c *fakeControl) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	body, _ := io.ReadAll(r.Body)
	c.mu.Lock()
	defer c.mu.Unlock()
	c.signed = append(c.signed, r.Clone(context.Background()))
	c.bodies = append(c.bodies, body)
	switch {
	case r.Method == http.MethodPost && r.URL.Path == requestClaimPath:
		var in map[string]any
		_ = json.Unmarshal(body, &in)
		c.claims = append(c.claims, in)
		var batch []claimedRequest
		if len(c.batches) > 0 {
			batch = c.batches[0]
			c.batches = c.batches[1:]
		}
		if batch == nil {
			batch = []claimedRequest{}
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"requests": batch}, "meta": map[string]any{}, "error": nil})
	case r.Method == http.MethodPost && strings.HasPrefix(r.URL.Path, "/v1/internal/schedules/requests/") && strings.HasSuffix(r.URL.Path, "/result"):
		id := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, "/v1/internal/schedules/requests/"), "/result")
		var out requestOutcome
		_ = json.Unmarshal(body, &out)
		c.results[id] = append(c.results[id], out)
		w.WriteHeader(http.StatusNoContent)
	default:
		http.NotFound(w, r)
	}
}

func (c *fakeControl) result(t *testing.T, id string) requestOutcome {
	t.Helper()
	c.mu.Lock()
	defer c.mu.Unlock()
	got := c.results[id]
	if len(got) != 1 {
		t.Fatalf("request %s: %d results posted, want 1 (%+v)", id, len(got), got)
	}
	return got[0]
}

func newTestRunner(t *testing.T, ctl http.Handler, temporal requestTemporal, env map[string]string) *requestRunner {
	t.Helper()
	srv := httptest.NewServer(ctl)
	t.Cleanup(srv.Close)
	m := New(nil, Config{ControlBaseURL: srv.URL, ControlHMACSecret: "shared-secret", HTTPClient: srv.Client()})
	r := m.newRequestRunner(temporal)
	r.worker = "orch-test"
	r.getenv = func(k string) string { return env[k] }
	r.now = func() time.Time { return time.Date(2026, 11, 3, 12, 0, 0, 0, time.UTC) }
	r.log = zerolog.Nop()
	return r
}

func TestRequestsEnabled_DefaultOff(t *testing.T) {
	for v, want := range map[string]bool{"": false, "0": false, "no": false, "1": true, "true": true, " On ": true} {
		if got := requestsEnabled(func(string) string { return v }); got != want {
			t.Fatalf("requestsEnabled(%q)=%v, want %v", v, got, want)
		}
	}
}

func TestRunRequests_TriggersAndPostsResult(t *testing.T) {
	ctl := newFakeControl([]claimedRequest{{ID: "req_1", ScheduleID: "sch_1", OrgID: "org_a", Action: "trigger", Attempts: 1}})
	tmp := &fakeTemporal{}
	r := newTestRunner(t, ctl, tmp, nil)

	r.tick(context.Background())

	if len(tmp.triggers) != 1 || tmp.triggers[0] != "sch_1" {
		t.Fatalf("triggers: %v", tmp.triggers)
	}
	out := ctl.result(t, "req_1")
	if out.Status != "done" || out.Worker != "orch-test" || out.Result["schedule_id"] != "sch_1" {
		t.Fatalf("outcome: %+v", out)
	}
	if len(ctl.claims) != 1 || ctl.claims[0]["worker"] != "orch-test" || ctl.claims[0]["lease_s"] != float64(requestLeaseS) {
		t.Fatalf("claim body: %v", ctl.claims)
	}
}

func TestRunRequests_BackfillPassesTheStoredWindow(t *testing.T) {
	ctl := newFakeControl([]claimedRequest{{
		ID: "req_b", ScheduleID: "sch_b", OrgID: "org_a", Action: "backfill", Attempts: 1,
		Params: map[string]any{"start_at": "2026-10-31T00:00:00Z", "end_at": "2026-11-03T00:00:00Z", "windows": 3},
	}})
	tmp := &fakeTemporal{}
	r := newTestRunner(t, ctl, tmp, nil)

	r.tick(context.Background())

	if len(tmp.backfills) != 1 || tmp.backfills[0] != "sch_b" {
		t.Fatalf("backfills: %v", tmp.backfills)
	}
	wantStart := time.Date(2026, 10, 31, 0, 0, 0, 0, time.UTC)
	wantEnd := time.Date(2026, 11, 3, 0, 0, 0, 0, time.UTC)
	if !tmp.windows[0][0].Equal(wantStart) || !tmp.windows[0][1].Equal(wantEnd) {
		t.Fatalf("window: %v", tmp.windows[0])
	}
	if out := ctl.result(t, "req_b"); out.Status != "done" {
		t.Fatalf("outcome: %+v", out)
	}
}

func TestRunRequests_BadBackfillParamsRejectedWithoutTemporal(t *testing.T) {
	ctl := newFakeControl([]claimedRequest{
		{ID: "req_x", ScheduleID: "sch_x", Action: "backfill", Attempts: 1, Params: map[string]any{"start_at": "nope"}},
		{ID: "req_y", ScheduleID: "sch_y", Action: "pause", Attempts: 1},
	})
	tmp := &fakeTemporal{}
	r := newTestRunner(t, ctl, tmp, nil)

	r.tick(context.Background())

	if len(tmp.backfills)+len(tmp.triggers) != 0 {
		t.Fatalf("Temporal was called: %v %v", tmp.triggers, tmp.backfills)
	}
	for _, id := range []string{"req_x", "req_y"} {
		if out := ctl.result(t, id); out.Status != "rejected" || out.Error == "" {
			t.Fatalf("%s outcome: %+v", id, out)
		}
	}
}

func TestRunRequests_TransientErrorRetriesThenFails(t *testing.T) {
	transient := errors.New("connection refused")
	ctl := newFakeControl(
		[]claimedRequest{{ID: "req_1", ScheduleID: "sch_1", Action: "trigger", Attempts: 1}},
		[]claimedRequest{{ID: "req_2", ScheduleID: "sch_2", Action: "trigger", Attempts: requestMaxAttempts}},
	)
	tmp := &fakeTemporal{errs: []error{transient, transient}}
	r := newTestRunner(t, ctl, tmp, nil)

	r.tick(context.Background())
	if out := ctl.result(t, "req_1"); out.Status != "retry" || !strings.Contains(out.Error, "connection refused") {
		t.Fatalf("first attempt: %+v", out)
	}
	r.tick(context.Background())
	if out := ctl.result(t, "req_2"); out.Status != "failed" {
		t.Fatalf("attempt %d: %+v, want failed", requestMaxAttempts, out)
	}
}

func TestRunRequests_UnknownScheduleRejected(t *testing.T) {
	ctl := newFakeControl([]claimedRequest{{ID: "req_1", ScheduleID: "sch_gone", Action: "trigger", Attempts: 1}})
	tmp := &fakeTemporal{errs: []error{
		// triggerWith wraps the SDK error; the classification must see through it.
		errors.Join(errors.New("trigger sch_gone"), serviceerror.NewNotFound("schedule not found")),
	}}
	r := newTestRunner(t, ctl, tmp, nil)

	r.tick(context.Background())

	if out := ctl.result(t, "req_1"); out.Status != "rejected" {
		t.Fatalf("outcome: %+v, want rejected", out)
	}
}

func TestRunRequests_ClaimFailureCallsNothing(t *testing.T) {
	tmp := &fakeTemporal{}
	r := newTestRunner(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, `{"error":{"code":"INTERNAL"}}`, http.StatusInternalServerError)
	}), tmp, nil)

	r.tick(context.Background())

	if len(tmp.triggers)+len(tmp.backfills) != 0 {
		t.Fatal("Temporal called after a failed claim")
	}
}

// Control verifies /v1/internal/schedules/ with the control-internal secret, so
// the loop signs with it when it is set (and with the shared one when not).
func TestRunRequests_SignsWithTheControlInternalSecret(t *testing.T) {
	for _, tc := range []struct {
		name string
		env  map[string]string
		want string
	}{
		{"control secret set", map[string]string{"QUARRY_CONTROL_INTERNAL_SECRET": "control-secret"}, "control-secret"},
		{"fallback to shared", nil, "shared-secret"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			ctl := newFakeControl()
			r := newTestRunner(t, ctl, &fakeTemporal{}, tc.env)
			r.tick(context.Background())
			if len(ctl.signed) != 1 {
				t.Fatalf("requests seen: %d", len(ctl.signed))
			}
			req, body := ctl.signed[0], ctl.bodies[0]
			sum := sha256.Sum256(body)
			canonical := req.Method + "\n" + req.URL.EscapedPath() + "\n" + hex.EncodeToString(sum[:]) + "\n" +
				req.Header.Get("X-Quarry-Sig-TS") + "\n" + req.Header.Get("X-Quarry-Sig-Nonce")
			mac := hmac.New(sha256.New, []byte(tc.want))
			_, _ = mac.Write([]byte(canonical))
			want := "sig_v1=" + base64.StdEncoding.EncodeToString(mac.Sum(nil))
			if got := req.Header.Get("X-Quarry-Sig"); got != want {
				t.Fatalf("signature %q, want one made with %q", got, tc.want)
			}
		})
	}
}

func TestRunRequests_LoopStopsOnCancel(t *testing.T) {
	ctl := newFakeControl()
	r := newTestRunner(t, ctl, &fakeTemporal{}, nil)
	r.interval = 10 * time.Millisecond
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- r.run(ctx) }()
	deadline := time.After(2 * time.Second)
	for {
		ctl.mu.Lock()
		n := len(ctl.claims)
		ctl.mu.Unlock()
		if n >= 2 {
			break
		}
		select {
		case <-deadline:
			t.Fatalf("the loop claimed %d times in 2 s", n)
		case <-time.After(5 * time.Millisecond):
		}
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("run returned %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("run did not stop on cancel")
	}
}
