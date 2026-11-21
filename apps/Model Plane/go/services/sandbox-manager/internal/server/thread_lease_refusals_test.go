package server

import (
	"context"
	"strings"
	"testing"
	"time"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/durationpb"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
)

// A thread lease that names a Space is refused with the switch off as well as on
// (D2-147). With the switch off it used to fall through to the Space path and leave a
// scope_type 'thread' row the thread index then held.
func TestThreadLeaseNamingASpaceIsRefusedWhateverTheSwitch(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		h := newThreadHarness(enabled, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
		h.s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
			return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
		}
		_, err := h.s.AcquireLease(context.Background(), &AcquireLeaseRequest{
			ScopeId: "thread-1", ScopeType: "thread", OrgId: "org-1", SpaceId: "space-1",
			CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
		})
		if enabled {
			wantCode(t, err, codes.InvalidArgument, "a thread lease naming a Space, switch on")
		} else {
			wantCode(t, err, codes.InvalidArgument, "a thread lease naming a Space, switch off")
		}
		if len(h.leases.byID) != 0 {
			t.Fatalf("switch %v: a refused thread lease wrote %d row(s)", enabled, len(h.leases.byID))
		}
	}
}

// The ZDR refusal on the thread RPCs comes before anything else, whatever the switch
// says and before the lease is read (D2-149). An unknown lease id therefore answers the
// ZDR refusal, not "not enabled" or NotFound.
func TestZDRRefusalComesFirstOnTheThreadRPCs(t *testing.T) {
	const want = "thread workspace is refused under zero data retention"
	for _, enabled := range []bool{false, true} {
		h := newThreadHarness(enabled, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})

		_, err := h.s.GetThreadWorkspace(context.Background(), &GetThreadWorkspaceRequest{
			LeaseId: "lease-missing", BackendId: "backend-1", Zdr: true,
		})
		wantCode(t, err, codes.FailedPrecondition, "ZDR GetThreadWorkspace, switch on or off")
		if msg := status.Convert(err).Message(); msg != want {
			t.Fatalf("switch %v: GetThreadWorkspace message = %q, want %q", enabled, msg, want)
		}

		_, err = h.s.ApplyThreadWorkspaceChanges(context.Background(), &ApplyThreadWorkspaceChangesRequest{
			LeaseId: "lease-missing", BackendId: "backend-1", Zdr: true,
			Changes: []*WorkspaceChangedFile{{Path: "a.txt", ContentHash: "sha256:a", SizeBytes: 1}},
		})
		wantCode(t, err, codes.FailedPrecondition, "ZDR ApplyThreadWorkspaceChanges, switch on or off")
		if msg := status.Convert(err).Message(); msg != want {
			t.Fatalf("switch %v: ApplyThreadWorkspaceChanges message = %q, want %q", enabled, msg, want)
		}
	}
}

// A lease acquired in an earlier, non-ZDR turn cannot read or write the thread for a
// ZDR call, and the refused write leaves the thread as it was. Erasure is not refused:
// a ZDR user can still remove what a thread holds (ZDR.md, the erasure rule).
func TestZDRCallerCannotReadOrCommitALiveThreadButCanErase(t *testing.T) {
	h, log, _, _ := erasureHarness(t, true)
	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire: %v", err)
	}
	id := lease.GetLeaseId()
	if _, err := h.apply(id, []*WorkspaceChangedFile{{Path: "kept.txt", ContentHash: "sha256:k", SizeBytes: 1}}, nil); err != nil {
		t.Fatalf("a non-ZDR apply: %v", err)
	}

	_, err = h.s.GetThreadWorkspace(context.Background(), &GetThreadWorkspaceRequest{LeaseId: id, BackendId: "backend-1", Zdr: true})
	wantCode(t, err, codes.FailedPrecondition, "ZDR GetThreadWorkspace on a live thread lease")
	_, err = h.s.ApplyThreadWorkspaceChanges(context.Background(), &ApplyThreadWorkspaceChangesRequest{
		LeaseId: id, BackendId: "backend-1", Zdr: true,
		Changes: []*WorkspaceChangedFile{{Path: "new.txt", ContentHash: "sha256:n", SizeBytes: 1}},
	})
	wantCode(t, err, codes.FailedPrecondition, "ZDR ApplyThreadWorkspaceChanges on a live thread lease")

	current, err := h.read(id)
	if err != nil {
		t.Fatalf("a non-ZDR read after the refusals: %v", err)
	}
	if len(current.GetEntries()) != 1 || current.GetEntries()[0].GetPath() != "kept.txt" {
		t.Fatalf("a refused ZDR apply changed the thread: %v", current.GetEntries())
	}

	if _, err := h.s.DeleteThreadWorkspace(context.Background(), &DeleteThreadWorkspaceRequest{ThreadId: "thread-1", OrgId: "org-1"}); err != nil {
		t.Fatalf("erasure refused for the thread: %v", err)
	}
	if got, want := strings.Join(log.events, "|"), "rows org-1/user-1/thread-1|blobs org-1/thread-1"; got != want {
		t.Fatalf("erase events = %q, want %q", got, want)
	}
}
