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
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
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
	return testServerFor(lease.NewStore(), snapshot.NewStore(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user",
	})
}

func testServerFor(leases *lease.Store, snapshots *snapshot.Store, principal authctx.Principal) *Server {
	srv := NewServer(leases, snapshots)
	srv.principal = func(context.Context) (authctx.Principal, error) { return principal, nil }
	return srv
}

func TestLeaseAccessIsPinnedToVerifiedOrganizationAndUser(t *testing.T) {
	leases := lease.NewStore()
	snapshots := snapshot.NewStore()
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
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		t.Fatal("capability verification must not run for a non-Space lease request")
		return authz.SpaceCapabilityClaims{}, nil
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
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		return authz.SpaceCapabilityClaims{}, errors.New("Space capability decision does not match the claimed lease request")
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
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		return authz.SpaceCapabilityClaims{BackendID: "backend-other"}, nil
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
	s.capabilityVerify = func(token, claimsJSON string, expect authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		gotExpectation = expect
		if token != "signed-token" || claimsJSON != "{}" {
			t.Fatalf("unexpected verify input: token=%q claims=%q", token, claimsJSON)
		}
		return authz.SpaceCapabilityClaims{BackendID: "backend-1"}, nil
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
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		return authz.SpaceCapabilityClaims{BackendID: "backend-1"}, nil
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
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		return authz.SpaceCapabilityClaims{BackendID: "backend-1"}, nil
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
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		return authz.SpaceCapabilityClaims{BackendID: "backend-1"}, nil
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
	s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error) {
		return authz.SpaceCapabilityClaims{BackendID: "backend-1"}, nil
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
