package server

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
)

// erasureRecorder keeps one ordered log of the rows and blob erasures a server
// asked for, so a test can see that the rows went before the blobs.
type erasureRecorder struct {
	events []string
}

// fakeRowErasure answers Erase with a fixed count or error and logs the call.
type fakeRowErasure struct {
	log   *erasureRecorder
	files int64
	err   error
}

func (f *fakeRowErasure) Erase(_ context.Context, orgID, ownerID, threadID string) (int64, error) {
	f.log.events = append(f.log.events, "rows "+orgID+"/"+ownerID+"/"+threadID)
	return f.files, f.err
}

// fakeBlobErasure answers DeleteThreadBlobs with a fixed error and logs the call.
type fakeBlobErasure struct {
	log *erasureRecorder
	err error
}

func (f *fakeBlobErasure) DeleteThreadBlobs(_ context.Context, orgID, threadID string) (int64, error) {
	f.log.events = append(f.log.events, "blobs "+orgID+"/"+threadID)
	return 0, f.err
}

func eraseRequest(orgID, threadID string) *DeleteThreadWorkspaceRequest {
	return &DeleteThreadWorkspaceRequest{OrgId: orgID, ThreadId: threadID}
}

func assertStatusCode(t *testing.T, err error, want codes.Code) {
	t.Helper()
	if got := status.Code(err); got != want {
		t.Fatalf("status = %v (err %v), want %v", got, err, want)
	}
}

func erasureHarness(t *testing.T, enabled bool) (*threadHarness, *erasureRecorder, *fakeRowErasure, *fakeBlobErasure) {
	t.Helper()
	h := newThreadHarness(enabled, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	rows := &fakeRowErasure{log: log, files: 3}
	blobs := &fakeBlobErasure{log: log}
	h.s.WithThreadErasure(rows, blobs)
	return h, log, rows, blobs
}

func TestDeleteThreadWorkspaceIsRefusedWhileTheSwitchIsOff(t *testing.T) {
	t.Parallel()
	h, log, _, _ := erasureHarness(t, false)

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.FailedPrecondition)
	if len(log.events) != 0 {
		t.Fatalf("erased with the switch off: %v", log.events)
	}
}

func TestDeleteThreadWorkspaceRefusesAServicePrincipal(t *testing.T) {
	t.Parallel()
	h, log, _, _ := erasureHarness(t, true)
	*h.who = authctx.Principal{OrganizationID: "org-1", ActorID: "control-plane", PrincipalType: "service"}

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.PermissionDenied)
	if len(log.events) != 0 {
		t.Fatalf("a service principal erased a thread: %v", log.events)
	}
}

func TestDeleteThreadWorkspaceRefusesAnotherOrganizationAndAMissingOne(t *testing.T) {
	t.Parallel()
	h, log, _, _ := erasureHarness(t, true)

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-2", "thread-1"))
	assertStatusCode(t, err, codes.PermissionDenied)
	_, err = h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("", "thread-1"))
	assertStatusCode(t, err, codes.InvalidArgument)
	if len(log.events) != 0 {
		t.Fatalf("erased across an organization boundary: %v", log.events)
	}
}

// TestDeleteThreadWorkspaceRefusesAnInvalidThreadIdBeforeAnyDeletion: an id that
// is not a plain key segment could name another thread's prefix.
func TestDeleteThreadWorkspaceRefusesAnInvalidThreadIdBeforeAnyDeletion(t *testing.T) {
	t.Parallel()
	h, log, _, _ := erasureHarness(t, true)

	for _, threadID := range []string{"", "thread/1", "../thread-1", "thread 1"} {
		_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", threadID))
		assertStatusCode(t, err, codes.InvalidArgument)
	}
	if len(log.events) != 0 {
		t.Fatalf("erased for an invalid thread id: %v", log.events)
	}
}

// TestDeleteThreadWorkspaceRefusesBeforeRemovingAnythingWithoutAnObjectStore: the
// rows must not go when the blobs could not follow them.
func TestDeleteThreadWorkspaceRefusesBeforeRemovingAnythingWithoutAnObjectStore(t *testing.T) {
	t.Parallel()
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	h.s.WithThreadErasure(&fakeRowErasure{log: log}, nil)

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.FailedPrecondition)
	if len(log.events) != 0 {
		t.Fatalf("removed rows with no object store wired: %v", log.events)
	}
}

// TestDeleteThreadWorkspaceErasesTheCallersThreadRowsThenItsBlobs covers the happy
// path: the caller's own identity keys the rows, and the rows go before the blobs.
func TestDeleteThreadWorkspaceErasesTheCallersThreadRowsThenItsBlobs(t *testing.T) {
	t.Parallel()
	h, log, _, _ := erasureHarness(t, true)

	resp, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	if err != nil {
		t.Fatalf("DeleteThreadWorkspace: %v", err)
	}
	if resp.GetFilesDeleted() != 3 || resp.GetObjectPrefix() != "cas/thread/org-1/thread-1/" {
		t.Fatalf("response = %d files at %q", resp.GetFilesDeleted(), resp.GetObjectPrefix())
	}
	want := "rows org-1/user-1/thread-1|blobs org-1/thread-1"
	if got := strings.Join(log.events, "|"); got != want {
		t.Fatalf("events = %q, want %q", got, want)
	}
}

// TestDeleteThreadWorkspaceReportsAFailedBlobRemovalForARetry: the rows are gone
// when the blobs fail, and the error tells the caller to retry, which finishes it.
func TestDeleteThreadWorkspaceReportsAFailedBlobRemovalForARetry(t *testing.T) {
	t.Parallel()
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	h.s.WithThreadErasure(&fakeRowErasure{log: log, files: 2}, &fakeBlobErasure{log: log, err: errors.New("connection reset")})

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.Internal)
	if !strings.Contains(err.Error(), "retry") {
		t.Fatalf("err = %v, want the retry instruction", err)
	}
	if len(log.events) != 2 {
		t.Fatalf("events = %v, want the rows and the blob attempt", log.events)
	}
}

// TestDeleteThreadWorkspaceNeverErasesAThreadAnotherOwnerHolds: a shared thread id
// is refused with nothing removed, so one user cannot delete another user's blobs.
func TestDeleteThreadWorkspaceNeverErasesAThreadAnotherOwnerHolds(t *testing.T) {
	t.Parallel()
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	h.s.WithThreadErasure(&fakeRowErasure{log: log, err: threadworkspace.ErrThreadShared}, &fakeBlobErasure{log: log})

	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.PermissionDenied)
	if len(log.events) != 1 {
		t.Fatalf("events = %v; the blobs must not be touched for a shared thread", log.events)
	}
}
