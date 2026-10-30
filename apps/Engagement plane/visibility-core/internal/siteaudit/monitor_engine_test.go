package siteaudit

import (
	"context"
	"errors"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

type monitorClock struct {
	mu    sync.Mutex
	value time.Time
}

func (c *monitorClock) now() time.Time          { c.mu.Lock(); defer c.mu.Unlock(); return c.value }
func (c *monitorClock) advance(d time.Duration) { c.mu.Lock(); c.value = c.value.Add(d); c.mu.Unlock() }
func monitored(t *testing.T, fail string) (*Service, *Monitor, *monitorClock, string) {
	t.Helper()
	s, _, _, file := testService(t, source())
	clock := &monitorClock{value: testNow}
	s.now = clock.now
	c := source()
	base := c.captureFn
	c.captureFn = func(ctx context.Context, site, raw string) (CapturePage, error) {
		p, err := base(ctx, site, raw)
		p.ObservedAt = stamp(clock.now())
		return p, err
	}
	if fail != "" {
		c.mapFn = func(context.Context, string) ([]discovery.Page, error) { return nil, &CaptureError{Reason: fail} }
	}
	s.capture = c
	m := NewMonitor(s)
	if _, err := m.Update(testID, testSite, 1, 0, true, "daily"); err != nil {
		t.Fatal(err)
	}
	return s, m, clock, file
}

func TestMonitoringRetriesAreBoundedAndPolicyErrorsBlock(t *testing.T) {
	for _, reason := range []string{"timeout", "quota_limited", "source_unavailable", "policy_blocked", "access_denied", "invalid_source"} {
		t.Run(reason, func(t *testing.T) {
			s, m, clock, _ := monitored(t, reason)
			clock.advance(24 * time.Hour)
			for attempt := 1; attempt <= 3; attempt++ {
				if err := m.Tick(); err != nil {
					t.Fatal(err)
				}
				finish(t, s)
				v, _ := m.Get(testID, testSite)
				if v.FailureCount != attempt || len(v.History) != attempt || v.History[0].Attempt != attempt || v.History[0].Reason != reason {
					t.Fatal(v)
				}
				if attempt == 3 || !transientMonitorFailure(reason) {
					if v.Enabled || v.Status != "blocked" || v.NextRunAt != "" {
						t.Fatal(v)
					}
					if err := m.Tick(); err != nil {
						t.Fatal(err)
					}
					again, _ := m.Get(testID, testSite)
					if len(again.History) != attempt {
						t.Fatal("blocked schedule fetched")
					}
					break
				}
				delay := 5 * time.Minute
				if attempt == 2 {
					delay = 30 * time.Minute
				}
				if v.NextRunAt != stamp(clock.now().Add(delay)) || v.Status != "active" {
					t.Fatal(v)
				}
				if err := m.Tick(); err != nil {
					t.Fatal(err)
				}
				again, _ := m.Get(testID, testSite)
				if len(again.History) != attempt {
					t.Fatal("retry ran before backoff")
				}
				clock.advance(delay)
			}
		})
	}
}

func TestMonitoringPreservesHomogeneousPageFailureWithoutPolicyRetries(t *testing.T) {
	for _, reason := range []string{"access_denied", "policy_blocked", "invalid_source", "unsupported_content"} {
		t.Run(reason, func(t *testing.T) {
			s, m, clock, _ := monitored(t, "")
			c := source()
			c.captureFn = func(context.Context, string, string) (CapturePage, error) {
				return CapturePage{}, &CaptureError{Reason: reason}
			}
			s.capture = c
			clock.advance(24 * time.Hour)
			if err := m.Tick(); err != nil {
				t.Fatal(err)
			}
			audit := finish(t, s)
			v, _ := m.Get(testID, testSite)
			if audit.Reason != reason || v.Status != "blocked" || v.Reason != reason || v.FailureCount != 1 {
				t.Fatal(audit.Reason, v)
			}
		})
	}
	if failedPagesReason(nil) != "source_unavailable" || failedPagesReason([]Page{{Status: "failed", Reason: "timeout"}, {Status: "failed", Reason: "access_denied"}}) != "source_unavailable" || failedPagesReason([]Page{{Status: "failed", Reason: "unsafe error"}}) != "source_unavailable" {
		t.Fatal("mixed/untrusted failure reason escaped")
	}
}

func TestMonitoringCapacityDefersWithoutFabricatedReceipt(t *testing.T) {
	s, m, clock, _ := monitored(t, "")
	clock.advance(24 * time.Hour)
	s.mu.Lock()
	s.active = 2
	s.mu.Unlock()
	if err := m.Tick(); err != nil {
		t.Fatal(err)
	}
	v, _ := m.Get(testID, testSite)
	if len(v.History) != 0 || v.LastAuditID != "" || v.Reason != "capacity_deferred" || v.NextRunAt != stamp(clock.now().Add(time.Minute)) {
		t.Fatal(v)
	}
	s.mu.Lock()
	s.active = 0
	s.mu.Unlock()
	clock.advance(time.Minute)
	if err := m.Tick(); err != nil {
		t.Fatal(err)
	}
	finish(t, s)
	v, _ = m.Get(testID, testSite)
	if len(v.History) != 1 || v.History[0].ScheduledFor != stamp(testNow.Add(24*time.Hour)) {
		t.Fatal(v)
	}
	// Completion must survive eviction of its full report by later manual audits.
	for range 7 {
		clock.advance(time.Minute)
		if _, err := s.Start(testID, testSite, 1); err != nil {
			t.Fatal(err)
		}
		finish(t, s)
	}
	v, _ = m.Get(testID, testSite)
	if len(v.History) != 1 || v.LastOutcome != "complete" {
		t.Fatal(v)
	}
	if _, err := s.GetRun(testID, testSite, v.LastAuditID); !errors.Is(err, ErrNotFound) {
		t.Fatal("fixture did not evict old full report", err)
	}
}

func TestMonitoringClaimFailureNeverDispatchesAndCompletionFailureRetriesOnlyStorage(t *testing.T) {
	s, m, clock, _ := monitored(t, "")
	clock.advance(24 * time.Hour)
	s.repo.beforePersist = func(diskState) error { return errors.New("unavailable") }
	if err := m.Tick(); err == nil {
		t.Fatal("claim persistence failure hidden")
	}
	v, _ := m.Get(testID, testSite)
	if len(v.History) != 0 || len(s.repo.list(testID, testSite)) != 0 {
		t.Fatal("uncommitted claim dispatched", v)
	}
	s.repo.beforePersist = nil
	failOnce := true
	s.repo.beforePersist = func(state diskState) error {
		if failOnce && len(state.Runs) > 0 && state.Runs[len(state.Runs)-1].Status != "running" {
			failOnce = false
			return errors.New("one terminal write failure")
		}
		return nil
	}
	if err := m.Tick(); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		s.mu.Lock()
		done := s.active == 0 && len(s.pendingTerminal) == 1
		s.mu.Unlock()
		if done {
			break
		}
		runtime.Gosched()
	}
	s.mu.Lock()
	pending := len(s.pendingTerminal)
	s.mu.Unlock()
	if pending != 1 {
		t.Fatal("terminal result not retained")
	}
	v, _ = m.Get(testID, testSite)
	if v.LastOutcome != "running" {
		t.Fatal(v)
	}
	if err := m.Tick(); err != nil {
		t.Fatal(err)
	}
	v, _ = m.Get(testID, testSite)
	if v.LastOutcome != "complete" || len(v.History) != 1 || v.NextRunAt != stamp(clock.now().Add(24*time.Hour)) {
		t.Fatal("terminal storage recovery failed", v)
	}
	if len(s.repo.list(testID, testSite)) != 1 {
		t.Fatal("storage retry recaptured")
	}
}

func TestMonitoringRestartAtomicallyRecoversClaimAndPauseIsPreserved(t *testing.T) {
	s, m, clock, file := monitored(t, "")
	clock.advance(24 * time.Hour)
	// Construct exactly the transaction written before a process could crash.
	v := s.repo.monitors()[0]
	audit := s.emptyWebsiteFixture()
	audit.ID = newID("audit_")
	audit.Status = "running"
	audit.StartedAt = stamp(clock.now())
	v.PendingFor, v.PendingAttempt = stamp(clock.now()), 1
	v.Revision++
	v.LastAuditID, v.LastOutcome, v.NextRunAt = audit.ID, "running", stamp(clock.now().Add(24*time.Hour))
	v.History = []MonitorReceipt{{AuditID: audit.ID, ScheduledFor: stamp(clock.now()), StartedAt: audit.StartedAt, Status: "running", Attempt: 1}}
	if err := s.repo.claimMonitor(v, audit, v.Revision-1); err != nil {
		t.Fatal(err)
	}
	repo, err := Open(file, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	restored := New(repo, s.registry, s.capture, clock.now)
	m = NewMonitor(restored)
	got, _ := m.Get(testID, testSite)
	if got.LastOutcome != "failed" || got.Reason != "interrupted" || got.History[0].Reason != "interrupted" || got.FailureCount != 1 {
		t.Fatal(got)
	}
	if err = m.Tick(); err != nil {
		t.Fatal(err)
	}
	after, _ := m.Get(testID, testSite)
	if len(after.History) != 1 {
		t.Fatal("restart duplicated before backoff")
	}
	clock.advance(5 * time.Minute)
	block := make(chan struct{})
	c := source()
	c.mapFn = func(context.Context, string) ([]discovery.Page, error) {
		<-block
		return nil, &CaptureError{Reason: "timeout"}
	}
	restored.capture = c
	if err = m.Tick(); err != nil {
		t.Fatal(err)
	}
	got, _ = m.Get(testID, testSite)
	if got.LastOutcome != "running" || len(got.History) != 2 || got.History[0].Attempt != 2 {
		t.Fatal(got)
	}
	if _, err = m.Update(testID, testSite, 1, got.Revision, true, "weekly"); !errors.Is(err, ErrCapacity) {
		t.Fatal("resumed while running", err)
	}
	got, err = m.Update(testID, testSite, 1, got.Revision, false, "weekly")
	if err != nil {
		t.Fatal(err)
	}
	if err = m.Tick(); err != nil {
		t.Fatal(err)
	}
	close(block)
	finish(t, restored)
	got, _ = m.Get(testID, testSite)
	if got.Enabled || got.Status != "paused" || got.Cadence != "weekly" || got.NextRunAt != "" {
		t.Fatal("completion overwrote explicit pause", got)
	}
}

func TestMonitoringCancelledPollingAndLostCaptureNeverFetch(t *testing.T) {
	s, m, clock, _ := monitored(t, "")
	clock.advance(24 * time.Hour)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := m.tick(ctx); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	done := make(chan struct{})
	go func() { m.Run(ctx); close(done) }()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("poller ignored cancellation")
	}
	if len(s.repo.list(testID, testSite)) != 0 {
		t.Fatal("cancelled poll fetched")
	}
	s.capture = nil
	if err := m.Tick(); err != nil {
		t.Fatal(err)
	}
	v, _ := m.Get(testID, testSite)
	if v.Status != "blocked" || v.Reason != "not_configured" || v.LastOutcome != "none" {
		t.Fatal(v)
	}
}

func TestMonitoringPollLoopDispatchesAndSurvivesPersistenceFailure(t *testing.T) {
	s, m, clock, _ := monitored(t, "")
	clock.advance(24 * time.Hour)
	failed := make(chan struct{})
	s.repo.beforePersist = func(diskState) error { close(failed); return errors.New("offline") }
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	ticks := make(chan time.Time)
	done := make(chan struct{})
	go func() { m.runPolling(ctx, ticks); close(done) }()
	ticks <- clock.now()
	<-failed
	s.repo.mu.Lock()
	s.repo.beforePersist = nil
	s.repo.mu.Unlock()
	ticks <- clock.now()
	// Sending a tick only transfers it to the polling goroutine; it does not
	// acknowledge the subsequent durable dispatch. Wait for that claim before
	// finish(), which legitimately returns immediately when the audit is idle.
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		value, _ := m.Get(testID, testSite)
		if value.LastOutcome != "none" {
			break
		}
		runtime.Gosched()
	}
	finish(t, s)
	v, _ := m.Get(testID, testSite)
	if len(v.History) != 1 || v.LastOutcome != "complete" {
		t.Fatal(v)
	}
	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("polling loop did not stop")
	}
}
