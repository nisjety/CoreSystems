package main

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// countingUpstream answers every call with success and counts the calls that reach
// it, so a test can see whether the guard let a call through.
type countingUpstream struct {
	searches  int
	downloads int
}

func (u *countingUpstream) Search(context.Context, doffin.SearchParams) (doffin.SearchResponse, error) {
	u.searches++
	return doffin.SearchResponse{NumHitsTotal: 1}, nil
}

func (u *countingUpstream) Download(context.Context, string) (doffin.Notice, error) {
	u.downloads++
	return doffin.Notice{ID: "2026-000001"}, nil
}

// While the lock is held, the guard forwards each call and its answer unchanged, and
// the run is not cancelled.
func TestLockGuardForwardsWhileLockHeld(t *testing.T) {
	up := &countingUpstream{}
	var lost atomic.Bool
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	g := lockGuard{up: up, lost: &lost, cancel: cancel}

	resp, err := g.Search(ctx, doffin.SearchParams{})
	if err != nil || resp.NumHitsTotal != 1 {
		t.Fatalf("Search = (%+v, %v), want the client's answer", resp, err)
	}
	n, err := g.Download(ctx, "2026-000001")
	if err != nil || n.ID != "2026-000001" {
		t.Fatalf("Download = (%+v, %v), want the client's answer", n, err)
	}
	if up.searches != 1 || up.downloads != 1 {
		t.Errorf("client calls = %d search, %d download, want 1 each", up.searches, up.downloads)
	}
	if ctx.Err() != nil {
		t.Error("the run was cancelled while the lock was held")
	}
}

// H2 (budget lock): after the fake pinger fails, the heartbeat sets the flag and
// cancels the run. The guard's next Search and Download return ErrLockLost, and the
// client is never called. The heartbeat runs every millisecond here, so the loss is
// seen at once.
func TestLockGuardRefusesEveryCallAfterPingFails(t *testing.T) {
	saved := lockHeartbeat
	lockHeartbeat = time.Millisecond
	t.Cleanup(func() { lockHeartbeat = saved })

	runCtx, cancelRun := context.WithCancel(context.Background())
	defer cancelRun()
	p := &fakePinger{err: errors.New("budget lock session is not alive")}
	watch := watchBudget(runCtx, cancelRun, p)
	select {
	case <-runCtx.Done():
	case <-time.After(5 * time.Second):
		t.Fatal("the heartbeat did not end the run after the ping failed")
	}
	if !watch.finish() {
		t.Fatal("the heartbeat did not record the lost lock")
	}

	up := &countingUpstream{}
	g := lockGuard{up: up, lost: &watch.lost, cancel: cancelRun}
	if _, err := g.Search(context.Background(), doffin.SearchParams{}); !errors.Is(err, ErrLockLost) {
		t.Errorf("Search after the lock was lost: error = %v, want ErrLockLost", err)
	}
	if _, err := g.Download(context.Background(), "2026-000001"); !errors.Is(err, ErrLockLost) {
		t.Errorf("Download after the lock was lost: error = %v, want ErrLockLost", err)
	}
	if up.searches != 0 || up.downloads != 0 {
		t.Errorf("client calls after the lock was lost = %d search, %d download, want none", up.searches, up.downloads)
	}
}

// A refused call cancels the run before it returns. The engine reads a cancelled run
// as a cancellation and writes no download attempt, so the refusal can never be
// recorded as a failed download, even if the heartbeat has not yet cancelled.
func TestLockGuardCancelsRunWhenRefusing(t *testing.T) {
	var lost atomic.Bool
	lost.Store(true)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	g := lockGuard{up: &countingUpstream{}, lost: &lost, cancel: cancel}

	if _, err := g.Download(ctx, "2026-000001"); !errors.Is(err, ErrLockLost) {
		t.Fatalf("Download error = %v, want ErrLockLost", err)
	}
	if ctx.Err() == nil {
		t.Error("the refused call did not cancel the run")
	}
}
