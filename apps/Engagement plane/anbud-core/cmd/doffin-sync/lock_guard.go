package main

import (
	"context"
	"errors"
	"fmt"
	"sync/atomic"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// ErrLockLost is returned by a Doffin call that is attempted once the budget lock is
// known to be lost. The call never reaches the client.
var ErrLockLost = errors.New("the budget lock was lost, so Doffin is not called")

// lockGuard is the Doffin side of a run that holds the budget lock. Each call checks
// lost first, which the heartbeat sets, so no request leaves for Doffin once the lock
// is gone. A refused call also cancels the run before it returns. The engine then
// reads the refusal as a cancellation and writes no download attempt, whatever the
// heartbeat's timing.
type lockGuard struct {
	up     upstream
	lost   *atomic.Bool
	cancel context.CancelFunc
}

// Search forwards a search while the budget lock is held.
func (g lockGuard) Search(ctx context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	if err := g.refusal("search"); err != nil {
		return doffin.SearchResponse{}, err
	}
	return g.up.Search(ctx, p)
}

// Download forwards a download while the budget lock is held.
func (g lockGuard) Download(ctx context.Context, id string) (doffin.Notice, error) {
	if err := g.refusal("download"); err != nil {
		return doffin.Notice{}, err
	}
	return g.up.Download(ctx, id)
}

// refusal returns the error for a call that must not run, or nil while the lock is
// held. A refusal cancels the run first.
func (g lockGuard) refusal(op string) error {
	if !g.lost.Load() {
		return nil
	}
	g.cancel()
	return fmt.Errorf("%s refused: %w", op, ErrLockLost)
}
