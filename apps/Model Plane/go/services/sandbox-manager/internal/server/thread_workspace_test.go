package server

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/durationpb"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/workspace"
)

// threadHarness is a Server over the in-memory stores whose caller can be
// switched between requests, so one test can play two users or a service.
type threadHarness struct {
	s      *Server
	leases *MemoryLeaseStore
	who    *authctx.Principal
}

func newThreadHarness(enabled bool, limits threadworkspace.Limits) *threadHarness {
	leases := NewMemoryLeaseStore()
	s := NewServer(leases, NewMemorySnapshotStore(), NewMemoryWorkspaceStore())
	h := &threadHarness{
		s:      s,
		leases: leases,
		who:    &authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"},
	}
	s.principal = func(context.Context) (authctx.Principal, error) { return *h.who, nil }
	s.backendID = "backend-1"
	s.WithThreadWorkspace(enabled, NewMemoryThreadWorkspaceStore(), limits)
	return h
}

func (h *threadHarness) acquireThread(threadID string, ttl time.Duration) (*AcquireLeaseResponse, error) {
	return h.s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: threadID, ScopeType: "thread", OrgId: "org-1", Ttl: durationpb.New(ttl),
	})
}

func (h *threadHarness) apply(leaseID string, changes []*WorkspaceChangedFile, deleted []*ThreadDeletedPath) (*ApplyThreadWorkspaceChangesResponse, error) {
	return h.s.ApplyThreadWorkspaceChanges(context.Background(), &ApplyThreadWorkspaceChangesRequest{
		LeaseId: leaseID, BackendId: "backend-1", Changes: changes, Deleted: deleted,
	})
}

func (h *threadHarness) read(leaseID string) (*GetThreadWorkspaceResponse, error) {
	return h.s.GetThreadWorkspace(context.Background(), &GetThreadWorkspaceRequest{LeaseId: leaseID, BackendId: "backend-1"})
}

func wantCode(t *testing.T, err error, want codes.Code, what string) {
	t.Helper()
	if status.Code(err) != want {
		t.Fatalf("%s: code = %v (err %v), want %v", what, status.Code(err), err, want)
	}
}

func TestThreadLeaseRejectsSpaceID(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	_, err := h.s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "thread-1", ScopeType: "thread", OrgId: "org-1", SpaceId: "space-1", Ttl: durationpb.New(time.Minute),
	})
	wantCode(t, err, codes.InvalidArgument, "thread lease naming a Space")
	if len(h.leases.byID) != 0 {
		t.Fatalf("a refused thread lease wrote %d row(s)", len(h.leases.byID))
	}
}

func TestThreadLeaseRefusesZDR(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	_, err := h.s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "thread-1", ScopeType: "thread", OrgId: "org-1", Zdr: true, Ttl: durationpb.New(time.Minute),
	})
	wantCode(t, err, codes.FailedPrecondition, "ZDR thread lease")
	if len(h.leases.byID) != 0 {
		t.Fatalf("a ZDR refusal wrote %d lease row(s)", len(h.leases.byID))
	}
}

func TestThreadWorkspaceRPCsRefuseSpaceLease(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	h.s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	spaceLease, err := h.s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "run-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease(Space): %v", err)
	}
	_, err = h.read(spaceLease.GetLeaseId())
	wantCode(t, err, codes.FailedPrecondition, "GetThreadWorkspace on a Space lease")
	_, err = h.apply(spaceLease.GetLeaseId(), []*WorkspaceChangedFile{{Path: "a.txt", ContentHash: "sha256:a", SizeBytes: 1}}, nil)
	wantCode(t, err, codes.FailedPrecondition, "ApplyThreadWorkspaceChanges on a Space lease")
}

func TestSpaceManifestNeverReadsThreadRows(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	thread, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire thread: %v", err)
	}
	if _, err := h.apply(thread.GetLeaseId(), []*WorkspaceChangedFile{{Path: "thread-only.txt", ContentHash: "sha256:t", SizeBytes: 1}}, nil); err != nil {
		t.Fatalf("apply thread file: %v", err)
	}
	if err := h.s.workspace.UpsertOverlay(context.Background(), "org-1", "space-1", "", []workspace.ChangedFile{
		{Path: "space.txt", ContentHash: "sha256:s", SizeBytes: 1},
	}); err != nil {
		t.Fatalf("seed Space row: %v", err)
	}
	h.s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	spaceLease, err := h.s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "run-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease(Space): %v", err)
	}
	manifest, err := h.s.GetWorkspaceManifest(context.Background(), &GetWorkspaceManifestRequest{
		LeaseId: spaceLease.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("GetWorkspaceManifest: %v", err)
	}
	if len(manifest.GetEntries()) != 1 || manifest.GetEntries()[0].GetPath() != "space.txt" {
		t.Fatalf("Space manifest = %v, want only the Space's own row and no thread row", manifest.GetEntries())
	}
}

func TestAnotherUserCannotAcquireOrReadAThreadLease(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	first, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("user-1 acquire: %v", err)
	}
	if _, err := h.apply(first.GetLeaseId(), []*WorkspaceChangedFile{{Path: "mine.txt", ContentHash: "sha256:m", SizeBytes: 1}}, nil); err != nil {
		t.Fatalf("user-1 apply: %v", err)
	}

	h.who = &authctx.Principal{OrganizationID: "org-1", ActorID: "user-2", PrincipalType: "user"}
	second, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("user-2 acquire: %v", err)
	}
	if second.GetLeaseId() == first.GetLeaseId() {
		t.Fatal("user-2 was handed user-1's thread lease")
	}
	_, err = h.read(first.GetLeaseId())
	wantCode(t, err, codes.NotFound, "user-2 reading user-1's lease")
	_, err = h.apply(first.GetLeaseId(), []*WorkspaceChangedFile{{Path: "x.txt", ContentHash: "sha256:x", SizeBytes: 1}}, nil)
	wantCode(t, err, codes.NotFound, "user-2 writing through user-1's lease")
	mine, err := h.read(second.GetLeaseId())
	if err != nil {
		t.Fatalf("user-2 reading own thread: %v", err)
	}
	if len(mine.GetEntries()) != 0 {
		t.Fatalf("user-2 sees %d file(s) of user-1's thread", len(mine.GetEntries()))
	}
}

func TestServicePrincipalCannotAcquireThreadLease(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	h.who = &authctx.Principal{OrganizationID: "org-1", ActorID: "service:execution-core", PrincipalType: "service"}
	_, err := h.acquireThread("thread-1", time.Minute)
	wantCode(t, err, codes.PermissionDenied, "service principal acquiring a thread lease")
	if len(h.leases.byID) != 0 {
		t.Fatalf("a refused service acquire wrote %d row(s)", len(h.leases.byID))
	}
}

func TestAcquireThreadIsIdempotentAndSlides(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	first, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("first acquire: %v", err)
	}
	second, err := h.acquireThread("thread-1", 30*time.Minute)
	if err != nil {
		t.Fatalf("second acquire: %v", err)
	}
	if second.GetLeaseId() != first.GetLeaseId() {
		t.Fatalf("second acquire minted %s, want the same lease %s", second.GetLeaseId(), first.GetLeaseId())
	}
	if !second.GetExpiresAt().AsTime().After(first.GetExpiresAt().AsTime()) {
		t.Fatalf("expiry did not slide: first %v, second %v", first.GetExpiresAt().AsTime(), second.GetExpiresAt().AsTime())
	}
	third, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("third acquire: %v", err)
	}
	if third.GetExpiresAt().AsTime().Before(second.GetExpiresAt().AsTime()) {
		t.Fatal("a shorter TTL shortened a live lease; expiry must only slide forward")
	}
	if len(h.leases.byID) != 1 {
		t.Fatalf("three acquires left %d rows, want one", len(h.leases.byID))
	}
}

func TestApplyReportsConflictWithoutOverwriting(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire: %v", err)
	}
	if _, err := h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{{Path: "a.txt", ContentHash: "sha256:one", SizeBytes: 3}}, nil); err != nil {
		t.Fatalf("first write: %v", err)
	}
	resp, err := h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{
		{Path: "a.txt", ContentHash: "sha256:two", SizeBytes: 3, BaseHash: "sha256:stale"},
	}, nil)
	if err != nil {
		t.Fatalf("stale write: %v", err)
	}
	if got := resp.GetConflictingPaths(); len(got) != 1 || got[0] != "a.txt" {
		t.Fatalf("conflicting_paths = %v, want [a.txt]", got)
	}
	current, err := h.read(lease.GetLeaseId())
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if len(current.GetEntries()) != 1 || current.GetEntries()[0].GetContentHash() != "sha256:one" {
		t.Fatalf("file = %v, want the first writer's content kept", current.GetEntries())
	}
}

func TestApplyEnforcesQuota(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 2, MaxBytes: 1 << 20})
	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire: %v", err)
	}
	if _, err := h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{
		{Path: "a.txt", ContentHash: "sha256:a", SizeBytes: 1},
		{Path: "b.txt", ContentHash: "sha256:b", SizeBytes: 1},
	}, nil); err != nil {
		t.Fatalf("two files under a two-file cap: %v", err)
	}
	_, err = h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{
		{Path: "c.txt", ContentHash: "sha256:c", SizeBytes: 1},
	}, nil)
	wantCode(t, err, codes.ResourceExhausted, "third file over a two-file cap")
	current, err := h.read(lease.GetLeaseId())
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	if len(current.GetEntries()) != 2 {
		t.Fatalf("a refused batch left %d files, want the two from before", len(current.GetEntries()))
	}
}

func TestThreadWorkspaceRPCsAreRefusedWhileTheSwitchIsOff(t *testing.T) {
	h := newThreadHarness(false, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	// With the switch off a thread acquire takes the pre-WS22 path, unchanged.
	lease, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire with the switch off: %v", err)
	}
	_, err = h.read(lease.GetLeaseId())
	wantCode(t, err, codes.FailedPrecondition, "GetThreadWorkspace while off")
	_, err = h.apply(lease.GetLeaseId(), []*WorkspaceChangedFile{{Path: "a.txt", ContentHash: "sha256:a", SizeBytes: 1}}, nil)
	wantCode(t, err, codes.FailedPrecondition, "ApplyThreadWorkspaceChanges while off")
	_, err = h.s.DeleteThreadWorkspace(context.Background(), &DeleteThreadWorkspaceRequest{ThreadId: "thread-1", OrgId: "org-1"})
	wantCode(t, err, codes.FailedPrecondition, "DeleteThreadWorkspace while off")
}

// TestDeleteThreadWorkspaceRefusesUntilErasureIsWired: with the switch on but no
// erasure path wired, the erase is refused before any row is touched (WS22-S5).
func TestDeleteThreadWorkspaceRefusesUntilErasureIsWired(t *testing.T) {
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	_, err := h.s.DeleteThreadWorkspace(context.Background(), &DeleteThreadWorkspaceRequest{ThreadId: "thread-1", OrgId: "org-1"})
	wantCode(t, err, codes.FailedPrecondition, "DeleteThreadWorkspace with the switch on and no erasure wired")
}
