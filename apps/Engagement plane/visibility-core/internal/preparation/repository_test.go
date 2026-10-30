package preparation

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func TestProtectedStorageAndCorruption(t *testing.T) {
	s, repo, _ := fixture(t, nil)
	_, _ = s.Queue(testID, testSite, 1)
	if _, err := Open(repo.path, "other-org"); err == nil {
		t.Fatal("cross organization accepted")
	}
	for _, path := range []string{"relative", filepath.Dir(repo.path) + "/../bad.json"} {
		if _, err := Open(path, "org-test"); err == nil {
			t.Fatalf("invalid path accepted %s", path)
		}
	}
	if _, err := Open(repo.path, "bad org"); err == nil {
		t.Fatal("invalidorg accepted")
	}
	link := filepath.Join(filepath.Dir(repo.path), "link.json")
	if err := os.Symlink(repo.path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(link, "org-test"); err == nil {
		t.Fatal("symlink accepted")
	}
	dirlink := filepath.Join(t.TempDir(), "linked")
	if err := os.Symlink(filepath.Dir(repo.path), dirlink); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(filepath.Join(dirlink, "preparation.json"), "org-test"); err == nil {
		t.Fatal("directory symlink accepted")
	}
	if err := os.Chmod(repo.path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(repo.path, "org-test"); err == nil {
		t.Fatal("public file accepted")
	}
	_ = os.Chmod(repo.path, 0600)
	original, err := os.ReadFile(repo.path)
	if err != nil {
		t.Fatal(err)
	}
	cases := [][]byte{[]byte("{"), append(append([]byte{}, original...), []byte("{}")...), []byte(strings.Repeat("x", maxBytes+1))}
	var fields map[string]any
	if err = json.Unmarshal(original, &fields); err != nil {
		t.Fatal(err)
	}
	fields["secret"] = "never"
	unknown, _ := json.Marshal(fields)
	cases = append(cases, unknown)
	for i, raw := range cases {
		if err = os.WriteFile(repo.path, raw, 0600); err != nil {
			t.Fatal(err)
		}
		if _, err = Open(repo.path, "org-test"); err == nil {
			t.Fatalf("corrupt case %d accepted", i)
		}
	}
	_ = os.Chmod(filepath.Dir(repo.path), 0755)
	if _, err = Open(repo.path, "org-test"); err == nil {
		t.Fatal("public directory accepted")
	}
}

func TestPersistenceFailureRollsBackAndCommittedSyncIsVisible(t *testing.T) {
	s, repo, _ := fixture(t, nil)
	original := repo.persistFn
	repo.persistFn = func(diskState) error { return errors.New("disk unavailable") }
	if _, err := s.Queue(testID, testSite, 1); err == nil {
		t.Fatal("write failure accepted")
	}
	v, _ := s.Get(testID, testSite)
	if v.Status != "idle" {
		t.Fatal("uncommitted published")
	}
	repo.persistFn = func(state diskState) error {
		if err := original(state); err != nil {
			return err
		}
		return errCommitted
	}
	if _, err := s.Queue(testID, testSite, 1); !errors.Is(err, errCommitted) {
		t.Fatalf("lost sync failure %v", err)
	}
	v, _ = s.Get(testID, testSite)
	if v.Status != "queued" {
		t.Fatal("renamed receipt hidden")
	}
}

func TestTerminalSaveFailureNeverRepeatsRunner(t *testing.T) {
	var calls atomic.Int32
	var fail atomic.Bool
	fail.Store(true)
	s, repo, _ := fixture(t, runnerFunc(func(context.Context, websites.Website, string) (StepResult, error) {
		calls.Add(1)
		return StepResult{Status: "complete"}, nil
	}))
	original := repo.persistFn
	repo.persistFn = func(state diskState) error {
		if fail.Load() && len(state.Jobs) > 0 && state.Jobs[0].Steps[0].Status == "complete" {
			return errors.New("storage unavailable")
		}
		return original(state)
	}
	_, _ = s.Queue(testID, testSite, 1)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	deadline := time.Now().Add(time.Second)
	for calls.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	time.Sleep(300 * time.Millisecond)
	if calls.Load() != 1 {
		t.Fatalf("provider repeated %d", calls.Load())
	}
	fail.Store(false)
	waitStatus(t, s, "complete")
	cancel()
	<-done
	if calls.Load() != 3 {
		t.Fatalf("wrong phase calls %d", calls.Load())
	}
}

func TestMeasuredReceiptSettlesAfterJobDeadlineWithoutRepeatingSource(t *testing.T) {
	var calls atomic.Int32
	var fail atomic.Bool
	fail.Store(true)
	s, repo, _ := fixture(t, runnerFunc(func(context.Context, websites.Website, string) (StepResult, error) {
		calls.Add(1)
		return StepResult{Status: "complete"}, nil
	}))
	s.timeout = 30 * time.Millisecond
	original := repo.persistFn
	repo.persistFn = func(state diskState) error {
		if fail.Load() && len(state.Jobs) > 0 && state.Jobs[0].Steps[0].Status == "complete" {
			return errors.New("storage temporarily unavailable")
		}
		return original(state)
	}
	_, _ = s.Queue(testID, testSite, 1)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	defer func() { cancel(); <-done }()
	deadline := time.Now().Add(time.Second)
	for calls.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	time.Sleep(60 * time.Millisecond)
	fail.Store(false)
	got := waitStatus(t, s, "partial")
	if got.Steps[0].Status != "complete" || got.Steps[1].Reason != "timeout" || calls.Load() != 1 {
		t.Fatalf("lost receipt or repeated source: %#v calls=%d", got, calls.Load())
	}
}

func TestInterruptedStepNeverReexecutesAfterRestart(t *testing.T) {
	s, repo, _ := fixture(t, nil)
	queued, _ := s.Queue(testID, testSite, 1)
	err := repo.update(func(state diskState) (diskState, error) {
		r := state.Jobs[0]
		r.Status = "running"
		r.StartedAt = r.QueuedAt
		r.Attempts = 1
		r.Steps[0] = Step{Key: stepKeys[0], Status: "complete", ObservedAt: r.QueuedAt}
		r.Steps[1] = Step{Key: stepKeys[1], Status: "running"}
		state.Jobs[0] = r
		return state, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	reopened, err := Open(repo.path, "org-test")
	if err != nil {
		t.Fatal(err)
	}
	var calls atomic.Int32
	resumed := New(reopened, s.registry, runnerFunc(func(_ context.Context, _ websites.Website, key string) (StepResult, error) {
		if key != "pagespeed_desktop" {
			t.Errorf("repeated old phase %s", key)
		}
		calls.Add(1)
		return StepResult{Status: "complete"}, nil
	}), time.Now)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { resumed.Run(ctx); close(done) }()
	got := waitStatus(t, resumed, "partial")
	cancel()
	<-done
	if got.JobID != queued.JobID || got.Steps[1].Reason != "interrupted" || calls.Load() != 1 {
		t.Fatalf("recovery %#v calls%d", got, calls.Load())
	}
}

func TestValidationRejectsInvalidAndCrossOrgBindings(t *testing.T) {
	s, repo, reg := fixture(t, nil)
	for _, tc := range []struct {
		id, site string
		revision int64
	}{{"bad", testSite, 1}, {testID, "http://triodelab.no/", 1}, {testID, testSite, 0}} {
		if _, err := s.Queue(tc.id, tc.site, tc.revision); err == nil {
			t.Fatalf("invalid queue accepted %#v", tc)
		}
	}
	if _, err := s.Get(testID, "https://other.no/"); !errors.Is(err, websites.ErrNotFound) {
		t.Fatalf("missing %v", err)
	}
	other := &Repository{org: "other-org"}
	cross := New(other, reg, nil, nil)
	if _, err := cross.Get(testID, testSite); !errors.Is(err, ErrInvalid) {
		t.Fatalf("crossorg %v", err)
	}
	if _, err := New(nil, reg, nil, nil).Get(testID, testSite); !errors.Is(err, ErrInvalid) {
		t.Fatal("nil repo allowed")
	}
	_, _ = s.Queue(testID, testSite, 1)
	state := repo.read()
	if err := validState(state); err != nil {
		t.Fatal(err)
	}
	mutations := []func(*record){func(r *record) { r.Source = "raw" }, func(r *record) { r.Attempts = 4 }, func(r *record) { r.Status = "madeup" }, func(r *record) { r.QueuedAt = "bad" }, func(r *record) { r.Steps[0].Key = "secret" }, func(r *record) { r.Steps[0].Status = "complete" }, func(r *record) {
		r.Steps[0].Status = "failed"
		r.Steps[0].Reason = "privateerror"
		r.Steps[0].ObservedAt = r.QueuedAt
	}, func(r *record) { r.Steps[0].Status = "running" }, func(r *record) { r.CompletedAt = r.QueuedAt }, func(r *record) { r.StartedAt = "bad" }}
	for i, change := range mutations {
		next := cloneState(state)
		change(&next.Jobs[0])
		if validState(next) == nil {
			t.Errorf("invalidstate mutation%d accepted", i)
		}
	}
}
