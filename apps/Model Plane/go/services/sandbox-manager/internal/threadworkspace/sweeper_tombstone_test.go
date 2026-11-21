package threadworkspace

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"slices"
	"strings"
	"testing"
	"time"
)

// F-f06-02: a blob removal that fails is kept as a tombstone and retried by the
// sweeper. These tests run on fakes; the SQL is in sweeper_tombstone_integration_test.go.

var pendingThread = ThreadKey{OrgID: "org_1", OwnerID: "user_1", ThreadID: "thr_pending"}

// TestSweepRetriesAPendingBlobErasureAndClosesItWhenItSucceeds: the tombstone of an
// erase whose blob removal failed is listed and retried, and closed on success. It
// is listed only before the one-minute settle cutoff, with the batch bound.
func TestSweepRetriesAPendingBlobErasureAndClosesItWhenItSucceeds(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle()
	rows.pending = []ThreadKey{pendingThread}
	blobs := &fakeBlobs{}
	now := time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)

	if _, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), now); err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if want := []string{"org_1/thr_pending"}; !slices.Equal(blobs.removed, want) {
		t.Fatalf("blobs removed = %v, want %v", blobs.removed, want)
	}
	if len(rows.completed) != 1 || rows.completed[0] != pendingThread {
		t.Fatalf("completed = %v, want the pending tombstone closed", rows.completed)
	}
	if len(rows.pendingCutoffs) != 1 || !rows.pendingCutoffs[0].Equal(now.Add(-blobErasureSettle)) {
		t.Fatalf("pending cutoffs = %v, want one listing before %s", rows.pendingCutoffs, now.Add(-blobErasureSettle))
	}
	if len(rows.pendingLimits) != 1 || rows.pendingLimits[0] != sweepBatch {
		t.Fatalf("pending limits = %v, want one listing of %d", rows.pendingLimits, sweepBatch)
	}
}

// TestSweepKeepsAPendingBlobErasureWhoseRemovalFailsAndCountsTheAttempt: a failed
// retry stays a tombstone, with its attempt recorded, for a later pass.
func TestSweepKeepsAPendingBlobErasureWhoseRemovalFailsAndCountsTheAttempt(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle()
	rows.pending = []ThreadKey{pendingThread}
	blobs := &fakeBlobs{err: errors.New("object store unavailable")}

	if _, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now()); err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if len(rows.completed) != 0 {
		t.Fatalf("completed = %v after a failed removal; the tombstone must stay", rows.completed)
	}
	if got := rows.attempts[pendingThread]; got != 1 {
		t.Fatalf("attempts = %d, want 1", got)
	}
}

// TestSweepDrainsPendingErasuresBeforeTheIdlePass: the retries run first, so a
// backlog is not starved by the idle erasures of the same pass.
func TestSweepDrainsPendingErasuresBeforeTheIdlePass(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	rows.pending = []ThreadKey{pendingThread}

	if _, err := NewSweeper(rows, &fakeBlobs{}, time.Hour).sweep(context.Background(), time.Now()); err != nil {
		t.Fatalf("sweep: %v", err)
	}
	if want := []string{"pending", "idle"}; !slices.Equal(rows.order, want) {
		t.Fatalf("listing order = %v, want %v", rows.order, want)
	}
}

// TestSweepClosesTheTombstoneOfAnIdleEraseWhenItsBlobsGo: an idle erase's tombstone,
// written by the erase itself, is closed once its blobs are removed in the same pass.
func TestSweepClosesTheTombstoneOfAnIdleEraseWhenItsBlobsGo(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	rows.files = 2
	blobs := &fakeBlobs{}

	erased, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now())
	if err != nil || erased != 1 {
		t.Fatalf("sweep = (%d, %v), want one erased", erased, err)
	}
	if len(rows.completed) != 1 || rows.completed[0] != idleThread {
		t.Fatalf("completed = %v, want the idle erase's tombstone closed", rows.completed)
	}
}

// TestSweepLeavesTheTombstoneOfAnIdleEraseWhoseBlobsFail: the rows are gone and the
// blobs are not; the tombstone the erase wrote is what keeps the retry possible, so
// nothing closes it here.
func TestSweepLeavesTheTombstoneOfAnIdleEraseWhoseBlobsFail(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	blobs := &fakeBlobs{err: errors.New("object store unavailable")}

	erased, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now())
	if err != nil || erased != 1 {
		t.Fatalf("sweep = (%d, %v), want one erased and no error", erased, err)
	}
	if len(rows.completed) != 0 {
		t.Fatalf("completed = %v after a failed removal; the tombstone must stay", rows.completed)
	}
}

// TestSweepRunsTheIdlePassWhenTheRetryListingFails: a failed listing of the retries
// is logged, and the erasures of the same pass still run.
func TestSweepRunsTheIdlePassWhenTheRetryListingFails(t *testing.T) {
	t.Parallel()
	rows := newFakeIdle(idleThread)
	rows.pendingErr = errors.New("connection reset")

	erased, err := NewSweeper(rows, &fakeBlobs{}, time.Hour).sweep(context.Background(), time.Now())
	if err != nil || erased != 1 {
		t.Fatalf("sweep = (%d, %v), want the idle thread erased despite the failed retry listing", erased, err)
	}
}

// TestSweepLogsNoBlobKeyOrPath: an object store error can name a blob key, so the
// retry logs the ids and the attempt count and never the error text.
func TestSweepLogsNoBlobKeyOrPath(t *testing.T) {
	// Not parallel: it swaps the process-wide default logger.
	var buf bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&buf, &slog.HandlerOptions{Level: slog.LevelDebug})))
	defer slog.SetDefault(previous)

	rows := newFakeIdle()
	rows.pending = []ThreadKey{pendingThread}
	blobs := &fakeBlobs{err: errors.New(`remove "cas/thread/org_1/thr_pending/0a1b2c3d4e5f": access denied`)}

	if _, err := NewSweeper(rows, blobs, time.Hour).sweep(context.Background(), time.Now()); err != nil {
		t.Fatalf("sweep: %v", err)
	}
	logged := buf.String()
	if strings.Contains(logged, "cas/thread") || strings.Contains(logged, "0a1b2c3d4e5f") {
		t.Fatalf("log names a blob key or path: %s", logged)
	}
	if !strings.Contains(logged, "org_1") || !strings.Contains(logged, "thr_pending") || !strings.Contains(logged, "attempts=1") {
		t.Fatalf("log = %s, want the org, the thread and the attempt count", logged)
	}
}
