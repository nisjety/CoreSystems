package server

import (
	"context"
	"errors"
	"net"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
	"google.golang.org/protobuf/types/known/durationpb"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/workspace"
)

func newTestClient(t *testing.T) mpv1.SandboxManagerClient {
	t.Helper()

	listener := bufconn.Listen(1024 * 1024)
	grpcServer := grpc.NewServer()
	Register(grpcServer, newTestServer())

	go func() {
		_ = grpcServer.Serve(listener)
	}()

	t.Cleanup(func() {
		grpcServer.Stop()
		_ = listener.Close()
	})

	conn, err := grpc.DialContext(
		context.Background(),
		"bufnet",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(context.Context, string) (net.Conn, error) {
			return listener.Dial()
		}),
	)
	if err != nil {
		t.Fatalf("DialContext: unexpected error: %v", err)
	}
	t.Cleanup(func() {
		_ = conn.Close()
	})

	return mpv1.NewSandboxManagerClient(conn)
}

func newTestServer() *Server {
	return testServerFor(NewMemoryLeaseStore(), NewMemorySnapshotStore(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user",
	})
}

func testServerFor(leases LeaseStore, snapshots SnapshotStore, principal authctx.Principal) *Server {
	srv := NewServer(leases, snapshots, NewMemoryWorkspaceStore())
	srv.principal = func(context.Context) (authctx.Principal, error) { return principal, nil }
	return srv
}

func TestLeaseAccessIsPinnedToVerifiedOrganizationAndUser(t *testing.T) {
	leases := NewMemoryLeaseStore()
	snapshots := NewMemorySnapshotStore()
	owner := testServerFor(leases, snapshots, authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"})
	acquired, err := owner.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatal(err)
	}

	for _, tc := range []struct {
		name      string
		principal authctx.Principal
	}{
		{name: "wrong tenant", principal: authctx.Principal{OrganizationID: "org-2", ActorID: "user-1", PrincipalType: "user"}},
		{name: "wrong user", principal: authctx.Principal{OrganizationID: "org-1", ActorID: "user-2", PrincipalType: "user"}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			other := testServerFor(leases, snapshots, tc.principal)
			if _, err := other.SnapshotSandbox(context.Background(), &SnapshotRequest{LeaseId: acquired.GetLeaseId(), Label: "x"}); status.Code(err) != codes.NotFound {
				t.Fatalf("snapshot code = %v, want NotFound", status.Code(err))
			}
			if _, err := other.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acquired.GetLeaseId()}); status.Code(err) != codes.NotFound {
				t.Fatalf("release code = %v, want NotFound", status.Code(err))
			}
		})
	}

	service := testServerFor(leases, snapshots, authctx.Principal{OrganizationID: "org-1", ActorID: "service:execution-core", PrincipalType: "service", Scopes: []string{"sandbox:write"}})
	if _, err := service.SnapshotSandbox(context.Background(), &SnapshotRequest{LeaseId: acquired.GetLeaseId(), Label: "service"}); err != nil {
		t.Fatalf("same-tenant scoped service snapshot: %v", err)
	}
}

func TestAcquireLeaseRejectsCallerSuppliedWrongOrganization(t *testing.T) {
	_, err := newTestServer().AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-2", Ttl: durationpb.New(time.Minute),
	})
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("code = %v, want PermissionDenied", status.Code(err))
	}
}

func TestHealth_ReturnsServing(t *testing.T) {
	s := newTestServer()
	resp, err := s.Health(context.Background(), &SandboxHealthRequest{})
	if err != nil {
		t.Fatalf("Health: unexpected error: %v", err)
	}
	if resp.Status != "SERVING" {
		t.Errorf("expected status SERVING, got %q", resp.Status)
	}
}

func TestAcquireLease_Success(t *testing.T) {
	s := newTestServer()
	resp, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(30 * time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if resp.GetLeaseId() == "" {
		t.Error("expected non-empty LeaseID")
	}
	if resp.GetEndpoint() == "" {
		t.Error("expected non-empty Endpoint")
	}
	if resp.GetExpiresAt() == nil || !resp.GetExpiresAt().AsTime().After(time.Now()) {
		t.Errorf("expected ExpiresAt in future, got %v", resp.GetExpiresAt())
	}
}

func TestReleaseLease_NotFound(t *testing.T) {
	s := newTestServer()
	_, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: "does-not-exist"})
	if got := status.Code(err); got != codes.NotFound {
		t.Errorf("expected NotFound, got %v (err=%v)", got, err)
	}
}

func TestAcquireLease_ValidatesRequiredFields(t *testing.T) {
	tests := []struct {
		name string
		req  *AcquireLeaseRequest
	}{
		{
			name: "missing scope_id",
			req:  &AcquireLeaseRequest{ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "missing scope_type",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", OrgId: "org-1", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "invalid scope_type",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", ScopeType: "workspace", OrgId: "org-1", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "missing org_id",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", ScopeType: "agent", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "missing ttl",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1"},
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			s := newTestServer()
			_, err := s.AcquireLease(context.Background(), tc.req)
			if got := status.Code(err); got != codes.InvalidArgument {
				t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
			}
		})
	}
}

func TestReleaseLease_Success(t *testing.T) {
	s := newTestServer()
	acq, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	resp, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acq.GetLeaseId()})
	if err != nil {
		t.Fatalf("ReleaseLease: unexpected error: %v", err)
	}
	if !resp.Released {
		t.Error("expected Released=true")
	}
}

func TestSnapshotSandbox_LeaseNotFound(t *testing.T) {
	s := newTestServer()
	_, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: "does-not-exist", Label: "test",
	})
	if got := status.Code(err); got != codes.NotFound {
		t.Errorf("expected NotFound, got %v (err=%v)", got, err)
	}
}

func TestReleaseLease_EmptyLeaseIDReturnsInvalidArgument(t *testing.T) {
	s := newTestServer()
	_, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
	}
}

func TestSnapshotSandbox_ValidatesRequiredFields(t *testing.T) {
	tests := []struct {
		name string
		req  *SnapshotRequest
	}{
		{name: "missing lease_id", req: &SnapshotRequest{Label: "checkpoint"}},
		{name: "missing label", req: &SnapshotRequest{LeaseId: "lease-1"}},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			s := newTestServer()
			_, err := s.SnapshotSandbox(context.Background(), tc.req)
			if got := status.Code(err); got != codes.InvalidArgument {
				t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
			}
		})
	}
}

func TestSnapshotSandbox_Success(t *testing.T) {
	s := newTestServer()
	acq, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	resp, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acq.GetLeaseId(), Label: "checkpoint-1",
	})
	if err != nil {
		t.Fatalf("SnapshotSandbox: unexpected error: %v", err)
	}
	if resp.GetSnapshotId() == "" {
		t.Error("expected non-empty SnapshotID")
	}
	if resp.GetObjectKey() == "" {
		t.Error("expected non-empty ObjectKey")
	}
}

func TestAcquireLeaseWithoutSpaceIDNeverConsultsCapabilityVerification(t *testing.T) {
	s := newTestServer()
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		t.Fatal("capability verification must not run for a non-Space lease request")
		return authz.VerifiedCapability{}, nil
	}
	if _, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	}); err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
}

func TestAcquireLeaseRejectsSpaceScopedRequestWhenCapabilityVerificationIsNotConfigured(t *testing.T) {
	s := newTestServer()
	_, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if got := status.Code(err); got != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", got)
	}
}

func TestAcquireLeaseRejectsAnInvalidCapabilityDecision(t *testing.T) {
	s := newTestServer()
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{}, errors.New("Space capability decision does not match the claimed lease request")
	}
	s.backendID = "backend-1"
	_, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if got := status.Code(err); got != codes.PermissionDenied {
		t.Fatalf("code = %v, want PermissionDenied", got)
	}
}

// TestAcquireLeaseFailsClosedWhenClaimedBackendMismatchesInstance is the
// design doc's "backend loss/downgrade" scenario (S3.2 close-out design,
// §5): a capability decision verified as authentic but pinned to a
// different backend than this instance must still be refused.
func TestAcquireLeaseFailsClosedWhenClaimedBackendMismatchesInstance(t *testing.T) {
	s := newTestServer()
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-other"}}, nil
	}
	s.backendID = "backend-1"
	_, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if got := status.Code(err); got != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", got)
	}
}

func TestAcquireLeaseGrantsASpaceScopedLeaseOnAMatchingVerifiedBackend(t *testing.T) {
	s := newTestServer()
	var gotExpectation authz.CapabilityExpectation
	s.capabilityVerify = func(token, claimsJSON string, expect authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		gotExpectation = expect
		if token != "signed-token" || claimsJSON != "{}" {
			t.Fatalf("unexpected verify input: token=%q claims=%q", token, claimsJSON)
		}
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"
	resp, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "signed-token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if resp.GetBackendId() != "backend-1" {
		t.Fatalf("BackendId = %q, want backend-1", resp.GetBackendId())
	}
	if resp.GetState() != mpv1.SandboxLifecycleState_SCRATCH {
		t.Fatalf("State = %v, want SCRATCH", resp.GetState())
	}
	if gotExpectation.OrgID != "org-1" || gotExpectation.SpaceRef != "space-1" || gotExpectation.SubjectID != "user-1" {
		t.Fatalf("verifier was not given the request's own identity: %+v", gotExpectation)
	}
}

func TestReleaseLeaseAndSnapshotSandboxEnforceTheBackendPinOnASpaceScopedLease(t *testing.T) {
	s := newTestServer()
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}

	if _, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acquired.GetLeaseId(), Label: "x", BackendId: "backend-wrong",
	}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("snapshot with wrong backend_id: code = %v, want FailedPrecondition", status.Code(err))
	}
	if _, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-wrong",
	}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("release with wrong backend_id: code = %v, want FailedPrecondition", status.Code(err))
	}

	if _, err := s.ActivateLease(context.Background(), &ActivateLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	}); err != nil {
		t.Fatalf("ActivateLease with matching backend_id: unexpected error: %v", err)
	}
	if _, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acquired.GetLeaseId(), Label: "x", BackendId: "backend-1",
	}); err != nil {
		t.Fatalf("snapshot with matching backend_id: unexpected error: %v", err)
	}
	if _, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	}); err != nil {
		t.Fatalf("release with matching backend_id: unexpected error: %v", err)
	}
}

func TestSnapshotSandboxRejectsAScratchSpaceScopedLease(t *testing.T) {
	s := newTestServer()
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	// Never activated: still SCRATCH, so nothing durable exists yet.
	_, err = s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acquired.GetLeaseId(), Label: "x", BackendId: "backend-1",
	})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestActivateLeasePromotesScratchToActiveAndIsIdempotent(t *testing.T) {
	s := newTestServer()
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	resp, err := s.ActivateLease(context.Background(), &ActivateLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("ActivateLease: unexpected error: %v", err)
	}
	if resp.GetState() != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("State = %v, want ACTIVE", resp.GetState())
	}
	// Idempotent: activating again is a no-op, not an error.
	if _, err := s.ActivateLease(context.Background(), &ActivateLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	}); err != nil {
		t.Fatalf("second ActivateLease: unexpected error: %v", err)
	}
}

func TestActivateLeaseFailsClosedOnWrongBackend(t *testing.T) {
	s := newTestServer()
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	_, err = s.ActivateLease(context.Background(), &ActivateLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-wrong",
	})
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestActivateLeaseRequiresLeaseID(t *testing.T) {
	s := newTestServer()
	_, err := s.ActivateLease(context.Background(), &ActivateLeaseRequest{})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", got)
	}
}

// TestGetWorkspaceManifestIsEmptyForANonSpaceLease and
// TestGetWorkspaceManifestAndSnapshotSandboxLayerTheOverlayOverTheSpace are
// 3.5.C's own named scenarios (design doc §8 item 3.5.C): a non-Space lease
// has no manifest concept at all, and a Space-scoped lease's SnapshotSandbox
// call durably records its changed files as an overlay that
// GetWorkspaceManifest then reports shadowing the Space's own rows.
func TestGetWorkspaceManifestIsEmptyForANonSpaceLease(t *testing.T) {
	s := newTestServer()
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	resp, err := s.GetWorkspaceManifest(context.Background(), &GetWorkspaceManifestRequest{
		LeaseId: acquired.GetLeaseId(),
	})
	if err != nil {
		t.Fatalf("GetWorkspaceManifest: unexpected error: %v", err)
	}
	if len(resp.GetEntries()) != 0 {
		t.Fatalf("entries = %+v, want none for a non-Space lease", resp.GetEntries())
	}
}

func TestGetWorkspaceManifestAndSnapshotSandboxLayerTheOverlayOverTheSpace(t *testing.T) {
	leases := NewMemoryLeaseStore()
	snapshots := NewMemorySnapshotStore()
	workspaceStore := NewMemoryWorkspaceStore()
	s := NewServer(leases, snapshots, workspaceStore)
	s.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}, nil
	}
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"

	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if _, err := s.ActivateLease(context.Background(), &ActivateLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	}); err != nil {
		t.Fatalf("ActivateLease: unexpected error: %v", err)
	}

	// Seed a Space-level row directly, as an earlier run's already-merged
	// state would appear (this test only exercises the read/write surface
	// this handler owns, not step 4's PromoteWorkspace merge).
	if err := workspaceStore.UpsertOverlay(context.Background(), "org-1", "space-1", "", []workspace.ChangedFile{
		{Path: "shared.txt", ContentHash: "sha256:space-shared", SizeBytes: 5},
		{Path: "space-only.txt", ContentHash: "sha256:space-only", SizeBytes: 7},
	}); err != nil {
		t.Fatalf("seed space row: %v", err)
	}

	// Before any snapshot, the manifest is just the Space's own rows.
	before, err := s.GetWorkspaceManifest(context.Background(), &GetWorkspaceManifestRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("GetWorkspaceManifest (before snapshot): %v", err)
	}
	if len(before.GetEntries()) != 2 {
		t.Fatalf("entries (before) = %+v, want the 2 seeded Space rows", before.GetEntries())
	}

	if _, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acquired.GetLeaseId(), Label: "x", BackendId: "backend-1",
		ChangedFiles: []*WorkspaceChangedFile{
			{Path: "shared.txt", ContentHash: "sha256:run-shared", SizeBytes: 9, BaseHash: "sha256:space-shared"},
			{Path: "run-only.txt", ContentHash: "sha256:run-only", SizeBytes: 3},
		},
	}); err != nil {
		t.Fatalf("SnapshotSandbox: unexpected error: %v", err)
	}

	after, err := s.GetWorkspaceManifest(context.Background(), &GetWorkspaceManifestRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("GetWorkspaceManifest (after snapshot): %v", err)
	}
	byPath := make(map[string]string, len(after.GetEntries()))
	for _, e := range after.GetEntries() {
		byPath[e.GetPath()] = e.GetContentHash()
	}
	if len(byPath) != 3 {
		t.Fatalf("entries (after) = %+v, want exactly 3 distinct paths", after.GetEntries())
	}
	if byPath["shared.txt"] != "sha256:run-shared" {
		t.Fatalf("shared.txt = %q, want the run overlay's hash to shadow the Space row", byPath["shared.txt"])
	}
	if byPath["space-only.txt"] != "sha256:space-only" {
		t.Fatalf("space-only.txt = %q, want the untouched Space row", byPath["space-only.txt"])
	}
	if byPath["run-only.txt"] != "sha256:run-only" {
		t.Fatalf("run-only.txt = %q, want the new overlay row", byPath["run-only.txt"])
	}

	// PromoteWorkspace (S3.3 step 4) now merges that same overlay into the
	// Space's own durable rows. shared.txt's base_hash (sha256:space-shared)
	// matches what the Space had, so it merges cleanly; run-only.txt had no
	// prior Space row at all, so it merges as a brand-new one.
	promoted, err := s.PromoteWorkspace(context.Background(), &PromoteWorkspaceRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("PromoteWorkspace: unexpected error: %v", err)
	}
	if len(promoted.GetConflictingPaths()) != 0 {
		t.Fatalf("conflicting_paths = %v, want none", promoted.GetConflictingPaths())
	}
	spaceEntries, err := workspaceStore.GetManifest(context.Background(), "org-1", "space-1", "")
	if err != nil {
		t.Fatalf("GetManifest (Space-only view): %v", err)
	}
	spaceByPath := make(map[string]string, len(spaceEntries))
	for _, e := range spaceEntries {
		spaceByPath[e.Path] = e.ContentHash
	}
	if spaceByPath["shared.txt"] != "sha256:run-shared" {
		t.Fatalf("Space row shared.txt = %q, want the merged run content", spaceByPath["shared.txt"])
	}
	if spaceByPath["run-only.txt"] != "sha256:run-only" {
		t.Fatalf("Space row run-only.txt = %q, want the newly-merged content", spaceByPath["run-only.txt"])
	}
}

// TestPromoteWorkspaceReportsConflictWithoutOverwriting and
// TestPromoteWorkspaceMergesNonConflictingPathsIndependently are the design
// doc's own named scenarios (§4, §7), exercised at the handler level.
func TestPromoteWorkspaceReportsConflictWithoutOverwriting(t *testing.T) {
	leases := NewMemoryLeaseStore()
	snapshots := NewMemorySnapshotStore()
	workspaceStore := NewMemoryWorkspaceStore()
	s := NewServer(leases, snapshots, workspaceStore)
	s.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}, nil
	}
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"

	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if err := workspaceStore.UpsertOverlay(context.Background(), "org-1", "space-1", "", []workspace.ChangedFile{
		{Path: "contested.txt", ContentHash: "sha256:winner", SizeBytes: 6},
	}); err != nil {
		t.Fatalf("seed space row: %v", err)
	}
	if err := workspaceStore.UpsertOverlay(context.Background(), "org-1", "space-1", acquired.GetLeaseId(), []workspace.ChangedFile{
		{Path: "contested.txt", ContentHash: "sha256:loser", SizeBytes: 9, BaseHash: "sha256:original"},
	}); err != nil {
		t.Fatalf("seed overlay row: %v", err)
	}

	resp, err := s.PromoteWorkspace(context.Background(), &PromoteWorkspaceRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("PromoteWorkspace: unexpected error: %v", err)
	}
	if got := resp.GetConflictingPaths(); len(got) != 1 || got[0] != "contested.txt" {
		t.Fatalf("conflicting_paths = %v, want [contested.txt]", got)
	}
	spaceEntries, err := workspaceStore.GetManifest(context.Background(), "org-1", "space-1", "")
	if err != nil {
		t.Fatalf("GetManifest: %v", err)
	}
	if len(spaceEntries) != 1 || spaceEntries[0].ContentHash != "sha256:winner" {
		t.Fatalf("Space row = %+v, want the original content untouched by the conflicting attempt", spaceEntries)
	}
}

func TestPromoteWorkspaceMergesNonConflictingPathsIndependently(t *testing.T) {
	leases := NewMemoryLeaseStore()
	snapshots := NewMemorySnapshotStore()
	workspaceStore := NewMemoryWorkspaceStore()
	s := NewServer(leases, snapshots, workspaceStore)
	s.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}, nil
	}
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"

	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if err := workspaceStore.UpsertOverlay(context.Background(), "org-1", "space-1", "", []workspace.ChangedFile{
		{Path: "stale.txt", ContentHash: "sha256:current", SizeBytes: 4},
	}); err != nil {
		t.Fatalf("seed space row: %v", err)
	}
	if err := workspaceStore.UpsertOverlay(context.Background(), "org-1", "space-1", acquired.GetLeaseId(), []workspace.ChangedFile{
		{Path: "new.txt", ContentHash: "sha256:brand-new", SizeBytes: 3},
		{Path: "stale.txt", ContentHash: "sha256:attempt", SizeBytes: 8, BaseHash: "sha256:stale-base"},
	}); err != nil {
		t.Fatalf("seed overlay rows: %v", err)
	}

	resp, err := s.PromoteWorkspace(context.Background(), &PromoteWorkspaceRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("PromoteWorkspace: unexpected error: %v", err)
	}
	if got := resp.GetConflictingPaths(); len(got) != 1 || got[0] != "stale.txt" {
		t.Fatalf("conflicting_paths = %v, want [stale.txt]", got)
	}
	spaceEntries, err := workspaceStore.GetManifest(context.Background(), "org-1", "space-1", "")
	if err != nil {
		t.Fatalf("GetManifest: %v", err)
	}
	byPath := make(map[string]string, len(spaceEntries))
	for _, e := range spaceEntries {
		byPath[e.Path] = e.ContentHash
	}
	if byPath["new.txt"] != "sha256:brand-new" {
		t.Fatalf("new.txt = %q, want the merged content -- a conflict elsewhere in the overlay must not block it", byPath["new.txt"])
	}
	if byPath["stale.txt"] != "sha256:current" {
		t.Fatalf("stale.txt = %q, want the original content untouched", byPath["stale.txt"])
	}
}

// TestPromoteWorkspaceStillWorksAfterReleaseLease is the whole reason
// PromoteWorkspace resolves its lease via GetAny rather than GetScoped:
// merging a run's overlay is deliberately never tied to (or blocked by) the
// lease's own release, and ReleaseLease only ever touches the leases row,
// never workspace_files.
func TestPromoteWorkspaceStillWorksAfterReleaseLease(t *testing.T) {
	leases := NewMemoryLeaseStore()
	snapshots := NewMemorySnapshotStore()
	workspaceStore := NewMemoryWorkspaceStore()
	s := NewServer(leases, snapshots, workspaceStore)
	s.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}, nil
	}
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	s.backendID = "backend-1"

	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if err := workspaceStore.UpsertOverlay(context.Background(), "org-1", "space-1", acquired.GetLeaseId(), []workspace.ChangedFile{
		{Path: "new.txt", ContentHash: "sha256:brand-new", SizeBytes: 3},
	}); err != nil {
		t.Fatalf("seed overlay row: %v", err)
	}
	if _, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	}); err != nil {
		t.Fatalf("ReleaseLease: unexpected error: %v", err)
	}

	// GetWorkspaceManifest uses GetScoped and must now fail closed -- the
	// exact gap GetAny exists to work around, confirmed here so a future
	// change to PromoteWorkspace can't silently regress onto GetScoped
	// without a test noticing the two now behave differently.
	if _, err := s.GetWorkspaceManifest(context.Background(), &GetWorkspaceManifestRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	}); status.Code(err) != codes.NotFound {
		t.Fatalf("GetWorkspaceManifest after release: code = %v, want NotFound", status.Code(err))
	}

	resp, err := s.PromoteWorkspace(context.Background(), &PromoteWorkspaceRequest{
		LeaseId: acquired.GetLeaseId(), BackendId: "backend-1",
	})
	if err != nil {
		t.Fatalf("PromoteWorkspace after release: unexpected error: %v", err)
	}
	if len(resp.GetConflictingPaths()) != 0 {
		t.Fatalf("conflicting_paths = %v, want none", resp.GetConflictingPaths())
	}
	spaceEntries, err := workspaceStore.GetManifest(context.Background(), "org-1", "space-1", "")
	if err != nil {
		t.Fatalf("GetManifest: %v", err)
	}
	if len(spaceEntries) != 1 || spaceEntries[0].ContentHash != "sha256:brand-new" {
		t.Fatalf("Space row = %+v, want the overlay merged even after release", spaceEntries)
	}
}

func TestPromoteWorkspaceRequiresLeaseID(t *testing.T) {
	s := newTestServer()
	if _, err := s.PromoteWorkspace(context.Background(), &PromoteWorkspaceRequest{}); status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
	}
}

func TestPromoteWorkspaceRejectsANonSpaceLease(t *testing.T) {
	s := newTestServer()
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if _, err := s.PromoteWorkspace(context.Background(), &PromoteWorkspaceRequest{
		LeaseId: acquired.GetLeaseId(),
	}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", status.Code(err))
	}
}

func TestSnapshotSandboxSkipsTheOverlayWriteForANonSpaceLease(t *testing.T) {
	// A non-Space snapshot with changed_files set (which no real caller
	// sends today, but the field is technically settable) must not attempt
	// to record an overlay a non-Space lease has no SpaceID to key it under
	// -- proven here via a workspace store stub that fails any write.
	s := newTestServer()
	s.workspace = failingWorkspaceStore{t: t}
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if _, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acquired.GetLeaseId(), Label: "x",
		ChangedFiles: []*WorkspaceChangedFile{{Path: "x.txt", ContentHash: "sha256:x", SizeBytes: 1}},
	}); err != nil {
		t.Fatalf("SnapshotSandbox: unexpected error: %v", err)
	}
}

type failingWorkspaceStore struct{ t *testing.T }

func (failingWorkspaceStore) GetManifest(context.Context, string, string, string) ([]workspace.ManifestEntry, error) {
	return nil, nil
}

func (f failingWorkspaceStore) UpsertOverlay(context.Context, string, string, string, []workspace.ChangedFile) error {
	f.t.Fatal("UpsertOverlay must not be called for a non-Space lease")
	return nil
}

func (f failingWorkspaceStore) Promote(context.Context, string, string, string) ([]string, error) {
	f.t.Fatal("Promote must not be called by this test")
	return nil, nil
}

// TestReleaseLeaseTransitionsStateToDestroyed and
// TestSnapshotAfterDestroyIsRejected are the S3.2 close-out design's own
// named "suspend/destroy" scenario (§5): a destroyed lease must reject
// further snapshot/activate, and a second release must not surprise a
// caller with NotFound.
func TestReleaseLeaseTransitionsStateToDestroyed(t *testing.T) {
	s := newTestServer()
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if _, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acquired.GetLeaseId()}); err != nil {
		t.Fatalf("ReleaseLease: unexpected error: %v", err)
	}
	// A second release of the same lease is a clean idempotent success.
	resp, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acquired.GetLeaseId()})
	if err != nil {
		t.Fatalf("second ReleaseLease: unexpected error: %v", err)
	}
	if !resp.GetReleased() {
		t.Fatal("second ReleaseLease should still report Released=true")
	}
}

func TestSnapshotAfterDestroyIsRejected(t *testing.T) {
	s := newTestServer()
	acquired, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if _, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acquired.GetLeaseId()}); err != nil {
		t.Fatalf("ReleaseLease: unexpected error: %v", err)
	}
	if _, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acquired.GetLeaseId(), Label: "x",
	}); status.Code(err) != codes.NotFound {
		t.Fatalf("snapshot after destroy: code = %v, want NotFound", status.Code(err))
	}
	if _, err := s.ActivateLease(context.Background(), &ActivateLeaseRequest{
		LeaseId: acquired.GetLeaseId(),
	}); status.Code(err) != codes.NotFound {
		t.Fatalf("activate after destroy: code = %v, want NotFound", status.Code(err))
	}
}

func TestAcquireLease_TransportRoundTrip(t *testing.T) {
	client := newTestClient(t)

	resp, err := client.AcquireLease(context.Background(), &mpv1.AcquireLeaseRequest{
		ScopeId:   "scope-transport",
		ScopeType: "agent",
		Ttl:       durationpb.New(time.Minute),
		OrgId:     "org-1",
	})
	if err != nil {
		t.Fatalf("AcquireLease transport: unexpected error: %v", err)
	}
	if resp.GetLeaseId() == "" {
		t.Error("expected non-empty lease_id")
	}
	if resp.GetEndpoint() == "" {
		t.Error("expected non-empty endpoint")
	}
	if resp.GetExpiresAt() == nil || resp.GetExpiresAt().AsTime().Before(time.Now()) {
		t.Fatalf("expected future expires_at, got %v", resp.GetExpiresAt())
	}
}
