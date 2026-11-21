package server

import (
	"context"
	"errors"
	"slices"
	"testing"

	"google.golang.org/grpc/codes"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
)

// F-f06-02 at the server: the tombstone the erase wrote is closed only once the
// blobs are gone. A blob failure leaves it for the retention sweeper, and a
// failure to close it does not fail an erase whose blobs are already removed.

// TestDeleteThreadWorkspaceClosesTheTombstoneAfterTheBlobsGo: on success the caller's
// own tombstone is closed.
func TestDeleteThreadWorkspaceClosesTheTombstoneAfterTheBlobsGo(t *testing.T) {
	t.Parallel()
	h, _, rows, _ := erasureHarness(t, true)

	if _, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1")); err != nil {
		t.Fatalf("DeleteThreadWorkspace: %v", err)
	}
	want := []threadworkspace.ThreadKey{{OrgID: "org-1", OwnerID: "user-1", ThreadID: "thread-1"}}
	if !slices.Equal(rows.completed, want) {
		t.Fatalf("completed = %v, want %v", rows.completed, want)
	}
}

// TestDeleteThreadWorkspaceKeepsTheTombstoneWhenTheBlobsFail: a blob failure keeps
// the tombstone and keeps the Internal status the caller is told to retry on.
func TestDeleteThreadWorkspaceKeepsTheTombstoneWhenTheBlobsFail(t *testing.T) {
	t.Parallel()
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	rows := &fakeRowErasure{log: log, files: 2}
	h.s.WithThreadErasure(rows, &fakeBlobErasure{log: log, err: errors.New("connection reset")})

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.Internal)
	if len(rows.completed) != 0 {
		t.Fatalf("completed = %v after a failed blob removal; the tombstone must stay", rows.completed)
	}
}

// TestDeleteThreadWorkspaceSucceedsWhenClosingTheTombstoneFails: the blobs are gone,
// so the erase reports its count even though the tombstone is left for the sweeper.
func TestDeleteThreadWorkspaceSucceedsWhenClosingTheTombstoneFails(t *testing.T) {
	t.Parallel()
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	rows := &fakeRowErasure{log: log, files: 2, completeErr: errors.New("connection reset")}
	h.s.WithThreadErasure(rows, &fakeBlobErasure{log: log})

	resp, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	if err != nil {
		t.Fatalf("DeleteThreadWorkspace: %v", err)
	}
	if resp.GetFilesDeleted() != 2 {
		t.Fatalf("files = %d, want 2", resp.GetFilesDeleted())
	}
}

// TestDeleteThreadWorkspaceWithTheSwitchOffStillNeedsTheObjectStore: erasure runs
// without the switch, but never without the object store, so the rows do not go
// when the blobs could not follow them.
func TestDeleteThreadWorkspaceWithTheSwitchOffStillNeedsTheObjectStore(t *testing.T) {
	t.Parallel()
	h := newThreadHarness(false, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	h.s.WithThreadErasure(&fakeRowErasure{log: log}, nil)

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.FailedPrecondition)
	if len(log.events) != 0 {
		t.Fatalf("removed rows with no object store wired: %v", log.events)
	}
}

// TestRetentionEraseClosesTheTombstoneAfterItsBlobsGo: the service twin (F-f10-08)
// closes the tombstone of the owner it erased, as the user path does.
func TestRetentionEraseClosesTheTombstoneAfterItsBlobsGo(t *testing.T) {
	t.Parallel()
	h, _, rows, _ := retentionHarness(t, true)

	if _, err := retentionErase(h, "org-1", "thread-1", "user-9"); err != nil {
		t.Fatalf("retention erase: %v", err)
	}
	want := []threadworkspace.ThreadKey{{OrgID: "org-1", OwnerID: "user-9", ThreadID: "thread-1"}}
	if !slices.Equal(rows.completed, want) {
		t.Fatalf("completed = %v, want %v", rows.completed, want)
	}
}

// TestRetentionEraseKeepsTheTombstoneWhenTheBlobsFail: the retention erase's blob
// failure leaves the tombstone for the sweeper too.
func TestRetentionEraseKeepsTheTombstoneWhenTheBlobsFail(t *testing.T) {
	t.Parallel()
	h, log, rows, _ := retentionHarness(t, true)
	h.s.WithThreadErasure(rows, &fakeBlobErasure{log: log, err: errors.New("connection reset")})

	if _, err := retentionErase(h, "org-1", "thread-1", "user-9"); err == nil {
		t.Fatal("retention erase with a failed blob removal succeeded")
	}
	if len(rows.completed) != 0 {
		t.Fatalf("completed = %v after a failed blob removal; the tombstone must stay", rows.completed)
	}
}
