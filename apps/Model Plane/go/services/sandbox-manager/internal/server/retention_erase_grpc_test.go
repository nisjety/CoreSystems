//go:build retention_erase_gen

package server

import (
	"context"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"
)

// TestRetentionEraseRPCAnswersWithTheRowCountOnly drives the generated RPC method
// and pins that it is the same erase as the body (rows, then blobs) and that the
// response carries a count and nothing else.
func TestRetentionEraseRPCAnswersWithTheRowCountOnly(t *testing.T) {
	t.Parallel()
	h, log, _, _ := retentionHarness(t, true)

	resp, err := h.s.EraseThreadWorkspaceForRetention(context.Background(), &EraseThreadWorkspaceForRetentionRequest{
		OrgId: "org-1", ThreadId: "thread-1", OwnerId: "user-9",
	})
	if err != nil {
		t.Fatalf("EraseThreadWorkspaceForRetention: %v", err)
	}
	if resp.GetFilesDeleted() != 3 {
		t.Fatalf("files_deleted = %d, want 3", resp.GetFilesDeleted())
	}
	want := "rows org-1/user-9/thread-1|blobs org-1/thread-1"
	if got := strings.Join(log.events, "|"); got != want {
		t.Fatalf("events = %q, want %q", got, want)
	}
}

// TestRetentionEraseRPCRefusesWithTheBodysCodes: the method adds no check of its
// own, so the switch, a user and another organization are refused as the body does.
func TestRetentionEraseRPCRefusesWithTheBodysCodes(t *testing.T) {
	t.Parallel()
	off, offLog, _, _ := retentionHarness(t, false)
	_, err := off.s.EraseThreadWorkspaceForRetention(context.Background(), &EraseThreadWorkspaceForRetentionRequest{OrgId: "org-1", ThreadId: "thread-1", OwnerId: "user-9"})
	assertStatusCode(t, err, codes.FailedPrecondition)

	on, onLog, _, _ := retentionHarness(t, true)
	_, err = on.s.EraseThreadWorkspaceForRetention(context.Background(), &EraseThreadWorkspaceForRetentionRequest{OrgId: "org-2", ThreadId: "thread-1", OwnerId: "user-9"})
	assertStatusCode(t, err, codes.PermissionDenied)

	if len(offLog.events) != 0 || len(onLog.events) != 0 {
		t.Fatalf("a refused call erased: %v %v", offLog.events, onLog.events)
	}
}
