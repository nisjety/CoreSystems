package server

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc/codes"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
)

// retentionService is the verified identity session-core's retention sweeper
// presents: a service principal holding only the narrow erase scope.
func retentionService() authctx.Principal {
	return authctx.Principal{
		OrganizationID: "org-1",
		ActorID:        "service:session-core",
		PrincipalType:  "service",
		Scopes:         []string{authz.ScopeThreadWorkspaceEraseAny},
	}
}

// retentionHarness is an erasure harness with the F-f10-08 switch as given and a
// service caller.
func retentionHarness(t *testing.T, switchOn bool) (*threadHarness, *erasureRecorder, *fakeRowErasure, *fakeBlobErasure) {
	t.Helper()
	h, log, rows, blobs := erasureHarness(t, true)
	h.s.WithServiceThreadErase(switchOn)
	*h.who = retentionService()
	return h, log, rows, blobs
}

func retentionErase(h *threadHarness, orgID, threadID, ownerID string) (int64, error) {
	return h.s.eraseThreadWorkspaceForRetention(context.Background(), orgID, threadID, ownerID)
}

// TestRetentionEraseRemovesTheOwnersRowsThenTheBlobs is the happy path: the
// service's own org and the sweeper's owner_id key the rows, the rows go before the
// blobs, and the answer is a count.
func TestRetentionEraseRemovesTheOwnersRowsThenTheBlobs(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, true)

	files, err := retentionErase(h, "org-1", "thread-1", "user-9")
	if err != nil {
		t.Fatalf("retention erase: %v", err)
	}
	if files != 3 {
		t.Fatalf("files = %d, want 3", files)
	}
	want := []string{"rows org-1/user-9/thread-1", "blobs org-1/thread-1"}
	if strings.Join(log.events, "|") != strings.Join(want, "|") {
		t.Fatalf("events = %v, want %v", log.events, want)
	}
}

// refusedWithNothingDeleted runs one refused call and checks that no erasure ran.
func refusedWithNothingDeleted(t *testing.T, h *threadHarness, log *erasureRecorder, want codes.Code, orgID, threadID, ownerID string) {
	t.Helper()
	_, err := retentionErase(h, orgID, threadID, ownerID)
	assertStatusCode(t, err, want)
	if len(log.events) != 0 {
		t.Fatalf("erased despite the refusal: %v", log.events)
	}
}

func TestRetentionEraseIsRefusedWhileTheSwitchIsOff(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, false)
	refusedWithNothingDeleted(t, h, log, codes.FailedPrecondition, "org-1", "thread-1", "user-9")
}

func TestRetentionEraseRefusesAUserPrincipal(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, true)
	// Even a user who somehow carries the scope is refused: the scope belongs to a service.
	*h.who = authctx.Principal{OrganizationID: "org-1", ActorID: "user-9", PrincipalType: "user", Scopes: []string{authz.ScopeThreadWorkspaceEraseAny}}
	refusedWithNothingDeleted(t, h, log, codes.PermissionDenied, "org-1", "thread-1", "user-9")
}

func TestRetentionEraseRefusesAServiceWithoutTheScope(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, true)
	// sandbox:write is not the erase scope.
	*h.who = authctx.Principal{OrganizationID: "org-1", ActorID: "service:execution-core", PrincipalType: "service", Scopes: []string{authz.ScopeWrite, authz.ScopeRead}}
	refusedWithNothingDeleted(t, h, log, codes.PermissionDenied, "org-1", "thread-1", "user-9")
}

func TestRetentionEraseRefusesAnotherOrganizationAndAMissingOne(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, true)
	refusedWithNothingDeleted(t, h, log, codes.PermissionDenied, "org-2", "thread-1", "user-9")
	refusedWithNothingDeleted(t, h, log, codes.PermissionDenied, "*", "thread-1", "user-9")
	refusedWithNothingDeleted(t, h, log, codes.InvalidArgument, "", "thread-1", "user-9")
}

// TestRetentionEraseRefusesAWildcardOrInvalidThreadIdAndAMissingOwner: an id that
// is not a plain key segment could name another thread's prefix, and rows cannot
// be selected without an owner.
func TestRetentionEraseRefusesAWildcardOrInvalidThreadIdAndAMissingOwner(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, true)
	for _, threadID := range []string{"", "*", "thread/1", "../thread-1", "thread 1"} {
		refusedWithNothingDeleted(t, h, log, codes.InvalidArgument, "org-1", threadID, "user-9")
	}
	refusedWithNothingDeleted(t, h, log, codes.InvalidArgument, "org-1", "thread-1", "")
	refusedWithNothingDeleted(t, h, log, codes.InvalidArgument, "org-1", "thread-1", "   ")
}

// TestRetentionEraseRefusesBeforeRemovingAnythingWithoutAnObjectStore: the rows
// must not go when the blobs could not follow them.
func TestRetentionEraseRefusesBeforeRemovingAnythingWithoutAnObjectStore(t *testing.T) {
	t.Parallel()
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 10, MaxBytes: 1 << 20})
	log := &erasureRecorder{}
	h.s.WithThreadErasure(&fakeRowErasure{log: log}, nil).WithServiceThreadErase(true)
	*h.who = retentionService()
	refusedWithNothingDeleted(t, h, log, codes.FailedPrecondition, "org-1", "thread-1", "user-9")
}

// TestRetentionEraseRefusesASharedThreadAndDeletesNoBlobs: a thread another user
// also holds is not erased, whoever names the owner; the rows step refuses and the
// blobs are never reached.
func TestRetentionEraseRefusesASharedThreadAndDeletesNoBlobs(t *testing.T) {
	t.Parallel()
	h, log, rows, _ := retentionHarness(t, true)
	rows.err = threadworkspace.ErrThreadShared

	_, err := retentionErase(h, "org-1", "thread-1", "user-9")
	assertStatusCode(t, err, codes.PermissionDenied)
	if !strings.Contains(err.Error(), "not yours to erase") {
		t.Fatalf("shared-thread message = %v", err)
	}
	if strings.Join(log.events, "|") != "rows org-1/user-9/thread-1" {
		t.Fatalf("a shared thread reached the blobs: %v", log.events)
	}
}

// TestRetentionEraseReportsABlobFailureSoARetryFinishes: the rows are already
// gone, so the error says to retry.
func TestRetentionEraseReportsABlobFailureSoARetryFinishes(t *testing.T) {
	t.Parallel()
	h, log, _, blobs := retentionHarness(t, true)
	blobs.err = errors.New("object store down")

	_, err := retentionErase(h, "org-1", "thread-1", "user-9")
	assertStatusCode(t, err, codes.Internal)
	if !strings.Contains(err.Error(), "retry the erase") {
		t.Fatalf("blob failure message = %v", err)
	}
	if len(log.events) != 2 || !strings.HasPrefix(log.events[0], "rows ") || !strings.HasPrefix(log.events[1], "blobs ") {
		t.Fatalf("events = %v", log.events)
	}
}

// TestDeleteThreadWorkspaceIsUnchangedByTheServiceSwitch: turning the new switch
// on must not let a service use the user-bound RPC.
func TestDeleteThreadWorkspaceIsUnchangedByTheServiceSwitch(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, true)
	_, err := h.s.DeleteThreadWorkspace(context.Background(), eraseRequest("org-1", "thread-1"))
	assertStatusCode(t, err, codes.PermissionDenied)
	if len(log.events) != 0 {
		t.Fatalf("the service switch opened the user RPC to a service: %v", log.events)
	}
}
