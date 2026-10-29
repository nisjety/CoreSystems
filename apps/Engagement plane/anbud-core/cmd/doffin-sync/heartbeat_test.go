package main

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"
)

// fakePinger answers okCalls pings with nil, then fails with err. beforeFail, when
// set, runs on the failing ping and receives the context the ping was given.
type fakePinger struct {
	okCalls    int
	err        error
	beforeFail func(context.Context)
	calls      int
}

func (p *fakePinger) Ping(ctx context.Context) error {
	p.calls++
	if p.calls <= p.okCalls {
		return nil
	}
	if p.beforeFail != nil {
		p.beforeFail(ctx)
	}
	return p.err
}

// H2: the heartbeat stops at the first failed ping and reports it once. The loop
// is pure over a pinger, so no database is needed. A five-second guard on the
// context ends a loop that never stops, so a broken loop fails instead of hanging.
func TestKeepLockAliveStopsAtFirstFailedPing(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	sessionGone := errors.New("budget lock session is not alive")
	p := &fakePinger{okCalls: 3, err: sessionGone}

	var lost []error
	keepLockAlive(ctx, p, time.Millisecond, lockPingTimeout, func(err error) { lost = append(lost, err) })

	if len(lost) != 1 {
		t.Fatalf("onLost called %d times, want exactly once", len(lost))
	}
	if !errors.Is(lost[0], sessionGone) {
		t.Errorf("onLost got %v, want the failed ping's error", lost[0])
	}
	if p.calls != 4 {
		t.Errorf("pings = %d, want 4: three that succeed and the one that fails", p.calls)
	}
}

// A lost budget lock is noticed within a second, so the heartbeat runs every second.
func TestLockHeartbeatIsOneSecond(t *testing.T) {
	if lockHeartbeat != time.Second {
		t.Errorf("lockHeartbeat = %v, want 1s", lockHeartbeat)
	}
}

// A ping gets its own timeout, longer than the heartbeat interval, so one slow ping is
// not read as a lost lock.
func TestLockPingTimeoutIsFiveSecondsAndLongerThanInterval(t *testing.T) {
	if lockPingTimeout != 5*time.Second {
		t.Errorf("lockPingTimeout = %v, want 5s", lockPingTimeout)
	}
	if lockPingTimeout <= lockHeartbeat {
		t.Errorf("lockPingTimeout = %v must exceed lockHeartbeat = %v", lockPingTimeout, lockHeartbeat)
	}
}

// A run that ends stops the heartbeat before any ping and reports nothing.
func TestKeepLockAliveStopsWhenRunEnds(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	p := &fakePinger{}

	keepLockAlive(ctx, p, time.Hour, lockPingTimeout, func(error) {
		t.Error("onLost called after the run ended")
	})
	if p.calls != 0 {
		t.Errorf("pings = %d after the run ended, want 0", p.calls)
	}
}

// A failed ping that comes while the run is ending is not reported as a lost lock.
// The ping itself must not be cut short by that shutdown, so its context stays live.
func TestKeepLockAliveDoesNotReportFailureDuringShutdown(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	var pingCtxErr error
	p := &fakePinger{
		err: errors.New("shut down while pinging"),
		beforeFail: func(pingCtx context.Context) {
			cancel()
			pingCtxErr = pingCtx.Err()
		},
	}

	reported := 0
	keepLockAlive(ctx, p, time.Millisecond, lockPingTimeout, func(error) { reported++ })

	if reported != 0 {
		t.Errorf("onLost called %d times during shutdown, want 0", reported)
	}
	if pingCtxErr != nil {
		t.Errorf("ping context ended with the run: %v; a shutdown must not cut a ping short", pingCtxErr)
	}
	if p.calls != 1 {
		t.Errorf("pings = %d, want 1", p.calls)
	}
}

// slowPinger answers each ping with nil after delay, or with the ping context's error
// if that context ends first. After stopAfter answers it calls stop, so the run ends
// on its own.
type slowPinger struct {
	delay     time.Duration
	stopAfter int
	calls     int
	stop      context.CancelFunc
}

func (p *slowPinger) Ping(ctx context.Context) error {
	p.calls++
	select {
	case <-time.After(p.delay):
	case <-ctx.Done():
		return ctx.Err()
	}
	if p.calls == p.stopAfter {
		p.stop()
	}
	return nil
}

// A ping slower than the heartbeat interval, but inside the ping timeout, is a live
// lock. It must not be read as lost. Here the interval is 5ms and each ping takes 30ms.
func TestKeepLockAliveSlowPingInsideTimeoutIsNotLost(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	p := &slowPinger{delay: 30 * time.Millisecond, stopAfter: 3, stop: cancel}

	var lost []error
	keepLockAlive(ctx, p, 5*time.Millisecond, 2*time.Second, func(err error) { lost = append(lost, err) })

	if len(lost) != 0 {
		t.Fatalf("onLost called %d times for a healthy slow ping, want none; first error: %v", len(lost), lost[0])
	}
	if p.calls < 3 {
		t.Errorf("pings = %d, want at least the 3 that succeed", p.calls)
	}
}

// blockingPinger never answers until its context ends. The five-second bound makes a
// missing ping timeout fail the test instead of hanging it.
type blockingPinger struct {
	calls int
}

func (p *blockingPinger) Ping(ctx context.Context) error {
	p.calls++
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-time.After(5 * time.Second):
		return errors.New("the ping was not bounded by a timeout")
	}
}

// A ping that outlasts the ping timeout is a lost lock. onLost is called once with the
// deadline error, and the loop ends after that one ping.
func TestKeepLockAlivePingPastTimeoutIsLostOnce(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	p := &blockingPinger{}

	var lost []error
	keepLockAlive(ctx, p, time.Millisecond, 20*time.Millisecond, func(err error) { lost = append(lost, err) })

	if len(lost) != 1 {
		t.Fatalf("onLost called %d times, want exactly once", len(lost))
	}
	if !errors.Is(lost[0], context.DeadlineExceeded) {
		t.Errorf("onLost got %v, want the ping timeout (context deadline exceeded)", lost[0])
	}
	if p.calls != 1 {
		t.Errorf("pings = %d, want 1: the first ping that times out ends the loop", p.calls)
	}
}

// steadyPinger answers every ping with nil after delay. It counts its pings atomically,
// because the heartbeat goroutine calls it while the test reads the count.
type steadyPinger struct {
	delay time.Duration
	calls atomic.Int32
}

func (p *steadyPinger) Ping(ctx context.Context) error {
	p.calls.Add(1)
	select {
	case <-time.After(p.delay):
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// The production wiring: watchBudget must give each ping the ping timeout, not the
// heartbeat interval. The heartbeat is 5ms here and each ping takes 30ms, so a ping
// bounded by the interval would be read as lost on the first tick.
func TestWatchBudgetSlowPingIsNotLost(t *testing.T) {
	saved := lockHeartbeat
	lockHeartbeat = 5 * time.Millisecond
	t.Cleanup(func() { lockHeartbeat = saved })

	runCtx, cancelRun := context.WithCancel(context.Background())
	defer cancelRun()
	p := &steadyPinger{delay: 30 * time.Millisecond}
	watch := watchBudget(runCtx, cancelRun, p)

	deadline := time.Now().Add(5 * time.Second)
	for p.calls.Load() < 3 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if lost := watch.finish(); lost {
		t.Fatal("a healthy slow ping was read as a lost lock")
	}
	if runCtx.Err() != nil {
		t.Error("the run was cancelled by a healthy slow ping")
	}
	if n := p.calls.Load(); n < 3 {
		t.Fatalf("pings = %d, want at least 3 before the watch stopped", n)
	}
}
