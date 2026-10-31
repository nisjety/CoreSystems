package main

import (
	"context"
	"sync/atomic"
	"time"
)

// lockHeartbeat is how often a running sync proves that it still holds the budget
// lock. It is a variable so a database test can shorten it.
var lockHeartbeat = 1 * time.Second

// lockPingTimeout bounds one ping of the budget lock. It is separate from the
// heartbeat interval: a ping slower than the interval is still a live lock, and only a
// ping that outlasts this timeout is read as lost.
const lockPingTimeout = 5 * time.Second

// pinger is the part of the budget lock the heartbeat needs. *rawstore.Lock
// satisfies it.
type pinger interface {
	Ping(ctx context.Context) error
}

// keepLockAlive pings p every interval until ctx ends. The first failed ping calls
// onLost once and ends the loop: once the lock's session is gone, another sync may
// hold the budget, so the run must stop calling Doffin. A failure that arrives
// while ctx is already ending is not reported, because the run is ending anyway.
// Each ping gets a context that ctx cannot cancel, bounded by timeout, so a shutdown
// never cuts a ping short and never closes the lock's session. A ping that outlasts
// timeout fails with the deadline error and is read as a lost lock.
func keepLockAlive(ctx context.Context, p pinger, every, timeout time.Duration, onLost func(error)) {
	ticker := time.NewTicker(every)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
		pingCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), timeout)
		err := p.Ping(pingCtx)
		cancel()
		if err == nil {
			continue
		}
		if ctx.Err() == nil {
			onLost(err)
		}
		return
	}
}

// budgetWatch runs keepLockAlive beside one sync. A lost lock cancels the run and
// is recorded, so the sync can end as cancelled. lost is the flag the heartbeat sets
// and the lockGuard reads before every Doffin call.
type budgetWatch struct {
	stop context.CancelFunc
	done chan struct{}
	lost atomic.Bool
}

// watchBudget starts the heartbeat for lock p. cancelRun ends the run when the lock
// is lost. The heartbeat keeps running through a shutdown until finish is called,
// so the lock is still watched while the run winds down.
func watchBudget(parent context.Context, cancelRun context.CancelFunc, p pinger) *budgetWatch {
	ctx, stop := context.WithCancel(context.WithoutCancel(parent))
	w := &budgetWatch{stop: stop, done: make(chan struct{})}
	go func() {
		defer close(w.done)
		keepLockAlive(ctx, p, lockHeartbeat, lockPingTimeout, func(error) {
			w.lost.Store(true)
			cancelRun()
		})
	}()
	return w
}

// finish stops the heartbeat, waits until it has returned, and reports whether the
// budget lock was lost. Calling it again returns the same answer.
func (w *budgetWatch) finish() bool {
	w.stop()
	<-w.done
	return w.lost.Load()
}
