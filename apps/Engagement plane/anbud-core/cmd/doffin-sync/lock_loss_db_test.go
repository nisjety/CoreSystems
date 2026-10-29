package main

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// The budget lock's advisory key is 0x646F666673796E63, split as pg_locks shows it:
// the high 32 bits are the class id and the low 32 bits the object id.
const (
	budgetClassID  int64 = 0x646F6666
	budgetObjectID int64 = 0x73796E63
)

// terminateBudgetHolder ends every session that holds the budget lock, except the
// one running this statement. It counts the sessions it ended.
const terminateBudgetHolder = `
SELECT count(*) FILTER (WHERE pg_terminate_backend(pid))
FROM pg_locks
WHERE locktype = 'advisory' AND granted AND objsubid = 1
  AND classid::bigint = $1 AND objid::bigint = $2
  AND pid <> pg_backend_pid()`

// killingUpstream ends the budget lock's session during its first search, the way a
// dropped connection or an operator's kill would. It then waits for the run to be
// cancelled. A run that is not cancelled fails the test after ten seconds rather than
// hanging it. Download must never be reached.
type killingUpstream struct {
	pool      *pgxpool.Pool
	searches  int
	downloads int
}

func (u *killingUpstream) Search(ctx context.Context, _ doffin.SearchParams) (doffin.SearchResponse, error) {
	u.searches++
	if u.searches == 1 {
		var killed int64
		if err := u.pool.QueryRow(context.Background(), terminateBudgetHolder, budgetClassID, budgetObjectID).Scan(&killed); err != nil {
			return doffin.SearchResponse{}, err
		}
		if killed != 1 {
			return doffin.SearchResponse{}, fmt.Errorf("ended %d budget sessions, want 1", killed)
		}
	}
	select {
	case <-ctx.Done():
		return doffin.SearchResponse{}, ctx.Err()
	case <-time.After(10 * time.Second):
		return doffin.SearchResponse{}, errors.New("the lost budget lock did not cancel the run")
	}
}

func (u *killingUpstream) Download(context.Context, string) (doffin.Notice, error) {
	u.downloads++
	return doffin.Notice{}, errors.New("download must not run after the budget lock is lost")
}

// Needs a throwaway database on ANBUD_TEST_DSN: run it with make test-db. The
// heartbeat notices that the budget lock's session
// has ended, cancels the run, and the run ends as cancelled with exit 5. The
// message names the budget lock, the summary reason is cancelled, and the lock
// is free afterwards.
func TestLockLossEndsRunCancelled(t *testing.T) {
	pool := dbtest.Open(t)
	saved := lockHeartbeat
	lockHeartbeat = 10 * time.Millisecond
	t.Cleanup(func() { lockHeartbeat = saved })

	d := loopbackDay()
	up := &killingUpstream{pool: pool}
	req := request{window: doffinsync.Window{From: d, To: d}, mode: doffinsync.ModeSync}
	code, stdout, stderr := invoke(t, pool, up, req)

	if code != exitCancelled {
		t.Fatalf("exit = %d, want %d (cancelled); stderr %q", code, exitCancelled, stderr)
	}
	if !strings.Contains(stderr, "budget lock was lost") {
		t.Errorf("stderr %q does not name the lost budget lock", stderr)
	}
	if up.downloads != 0 {
		t.Errorf("downloads = %d after the lock was lost, want 0", up.downloads)
	}
	objs := jsonLines(t, stdout)
	if len(objs) == 0 {
		t.Fatalf("stdout has no summary: %q", stdout)
	}
	if last := objs[len(objs)-1]; last["reason"] != "cancelled" {
		t.Errorf("summary reason = %v, want cancelled", last["reason"])
	}

	lock, err := rawstore.New(pool).TryLock(context.Background())
	if err != nil {
		t.Fatalf("the budget lock is still held after the lost run: %v", err)
	}
	if err := lock.Release(context.Background()); err != nil {
		t.Fatalf("release the lock taken to check it was free: %v", err)
	}
}
