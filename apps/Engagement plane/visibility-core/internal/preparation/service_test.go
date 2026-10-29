package preparation

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const testID = "web_11111111111111111111111111111111"
const testSite = "https://triodelab.no/"

type registryFixture struct {
	mu       sync.Mutex
	revision int64
}

func (r *registryFixture) OrganizationID() string { return "org-test" }
func (r *registryFixture) Resolve(id, site string) (websites.Record, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if id != testID || site != testSite {
		return websites.Record{}, websites.ErrNotFound
	}
	return websites.Record{Website: websites.Website{ID: id, Site: site, Revision: r.revision}}, nil
}
func (r *registryFixture) change() { r.mu.Lock(); defer r.mu.Unlock(); r.revision++ }

type runnerFunc func(context.Context, websites.Website, string) (StepResult, error)

func (f runnerFunc) Run(ctx context.Context, w websites.Website, key string) (StepResult, error) {
	return f(ctx, w, key)
}
func fixture(t *testing.T, runner Runner) (*Service, *Repository, *registryFixture) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := Open(filepath.Join(dir, "preparation.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	reg := &registryFixture{revision: 1}
	return New(repo, reg, runner, time.Now), repo, reg
}
func waitStatus(t *testing.T, s *Service, status string) Snapshot {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		v, err := s.Get(testID, testSite)
		if err != nil {
			t.Fatal(err)
		}
		if v.Status == status {
			return v
		}
		time.Sleep(5 * time.Millisecond)
	}
	v, _ := s.Get(testID, testSite)
	t.Fatalf("wanted %s, got %#v", status, v)
	return Snapshot{}
}
func TestQueueDurableIdempotentAndGetNeverRuns(t *testing.T) {
	s, repo, _ := fixture(t, nil)
	idle, err := s.Get(testID, testSite)
	if err != nil || idle.Status != "idle" || len(idle.Steps) != 3 {
		t.Fatalf("idle %#v %v", idle, err)
	}
	a, err := s.Queue(testID, testSite, 1)
	if err != nil {
		t.Fatal(err)
	}
	b, err := s.Queue(testID, testSite, 1)
	if err != nil || a.JobID != b.JobID {
		t.Fatal("queue not idempotent")
	}
	opened, err := Open(repo.path, "org-test")
	if err != nil {
		t.Fatal(err)
	}
	restarted := New(opened, s.registry, nil, time.Now)
	got, _ := restarted.Get(testID, testSite)
	if got.JobID != a.JobID || got.Status != "queued" {
		t.Fatalf("lost durable job %#v", got)
	}
	a.Steps[0].Status = "failed"
	fresh, _ := s.Get(testID, testSite)
	if fresh.Steps[0].Status != "queued" {
		t.Fatal("mutable snapshot escaped")
	}
	if _, err = s.Queue(testID, testSite, 2); !errors.Is(err, ErrConflict) {
		t.Fatalf("revision %v", err)
	}
}
func TestWorkerCompletesPartialAndCooldown(t *testing.T) {
	var mu sync.Mutex
	keys := []string{}
	s, _, _ := fixture(t, runnerFunc(func(_ context.Context, _ websites.Website, key string) (StepResult, error) {
		mu.Lock()
		keys = append(keys, key)
		mu.Unlock()
		if key == "pagespeed_mobile" {
			return StepResult{Status: "failed", Reason: "quota_limited"}, nil
		}
		return StepResult{Status: "complete"}, nil
	}))
	_, err := s.Queue(testID, testSite, 1)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	got := waitStatus(t, s, "partial")
	cancel()
	<-done
	if got.CompletedAt == "" || got.Steps[1].Reason != "quota_limited" {
		t.Fatalf("invalid result %#v", got)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(keys) != 3 {
		t.Fatalf("calls %v", keys)
	}
	if _, err = s.Queue(testID, testSite, 1); !errors.Is(err, ErrCooldown) {
		t.Fatalf("cooldown %v", err)
	}
}
func TestChangedBindingNeverPublishesOldResult(t *testing.T) {
	started := make(chan struct{})
	release := make(chan struct{})
	calls := 0
	s, _, reg := fixture(t, runnerFunc(func(context.Context, websites.Website, string) (StepResult, error) {
		calls++
		close(started)
		<-release
		return StepResult{Status: "complete"}, nil
	}))
	_, _ = s.Queue(testID, testSite, 1)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	<-started
	reg.change()
	close(release)
	time.Sleep(30 * time.Millisecond)
	got, err := s.Get(testID, testSite)
	cancel()
	<-done
	if err != nil || got.Status != "idle" || got.WebsiteRevision != 2 || got.JobID != "" || calls != 1 {
		t.Fatalf("stale result leaked %#v %v calls%d", got, err, calls)
	}
}
func TestMissingRunnerAndCancellation(t *testing.T) {
	s, _, _ := fixture(t, nil)
	_, _ = s.Queue(testID, testSite, 1)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	got := waitStatus(t, s, "failed")
	cancel()
	<-done
	for _, step := range got.Steps {
		if step.Status != "unavailable" || step.Reason != "not_configured" {
			t.Fatalf("invented result %#v", step)
		}
	}
}

type registryMany struct{}

func (registryMany) OrganizationID() string { return "org-test" }
func (registryMany) Resolve(id, site string) (websites.Record, error) {
	return websites.Record{Website: websites.Website{ID: id, Site: site, Revision: 1}}, nil
}
func TestWorkersAreBoundedAndQueueHasSiteLimit(t *testing.T) {
	s, _, _ := fixture(t, nil)
	s.registry = registryMany{}
	started := make(chan struct{}, 3)
	release := make(chan struct{})
	var active, maxActive atomic.Int32
	s.runner = runnerFunc(func(ctx context.Context, _ websites.Website, _ string) (StepResult, error) {
		n := active.Add(1)
		defer active.Add(-1)
		for {
			old := maxActive.Load()
			if n <= old || maxActive.CompareAndSwap(old, n) {
				break
			}
		}
		select {
		case started <- struct{}{}:
		default:
		}
		select {
		case <-release:
		case <-ctx.Done():
			return StepResult{}, ctx.Err()
		}
		return StepResult{Status: "complete"}, nil
	})
	for i := 1; i <= maxJobs; i++ {
		if _, err := s.Queue(fmt.Sprintf("web_%032x", i), fmt.Sprintf("https://site%d.no/", i), 1); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := s.Queue(testID, testSite, 1); !errors.Is(err, ErrCapacity) {
		t.Fatalf("unbounded jobs %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	for range 2 {
		select {
		case <-started:
		case <-time.After(time.Second):
			t.Fatal("workers did not start")
		}
	}
	select {
	case <-started:
		t.Fatal("third concurrent source started")
	case <-time.After(30 * time.Millisecond):
	}
	cancel()
	close(release)
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("workers leaked")
	}
	if maxActive.Load() != 2 {
		t.Fatalf("maxactive %d", maxActive.Load())
	}
}

func TestRecoveryBudgetAndInvalidRunnerResult(t *testing.T) {
	s, repo, _ := fixture(t, runnerFunc(func(context.Context, websites.Website, string) (StepResult, error) {
		return StepResult{Status: "complete", Reason: "private"}, nil
	}))
	_, _ = s.Queue(testID, testSite, 1)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	got := waitStatus(t, s, "failed")
	cancel()
	<-done
	if got.Steps[0].Reason != "source_unavailable" {
		t.Fatal("raw runner reason escaped")
	}
	err := repo.update(func(state diskState) (diskState, error) {
		r := state.Jobs[0]
		r.Status = "running"
		r.CompletedAt = ""
		r.Attempts = 3
		for i, key := range stepKeys {
			r.Steps[i] = Step{Key: key, Status: "queued"}
		}
		r.Steps[0].Status = "running"
		state.Jobs[0] = r
		return state, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if err = s.recover(); err != nil {
		t.Fatal(err)
	}
	got, _ = s.Get(testID, testSite)
	if got.Status != "failed" || got.Steps[2].Reason != "interrupted" {
		t.Fatalf("recoverybudget %#v", got)
	}
}
