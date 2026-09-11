package server

import (
	"context"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// Server implements SandboxManagerServer over LeaseStore/SnapshotStore —
// the real Postgres-backed implementations in production
// (lease.Store/snapshot.Store), a fast in-memory fake in this package's own
// tests (see server_test.go).
type Server struct {
	mpv1.UnimplementedSandboxManagerServer
	leases    LeaseStore
	snapshots SnapshotStore
	principal func(context.Context) (authctx.Principal, error)
	// capabilityVerify verifies a Space capability decision + its unsigned
	// claims sidecar, mirroring authz.SpaceCapabilityVerifier.Verify. A
	// func field, not the concrete type, so tests can inject a fake without
	// a real Ed25519 keypair — the same pattern as principal above. Nil
	// means "not configured": any AcquireLease request naming a space_id is
	// then refused, never served unverified.
	capabilityVerify func(token, claimsJSON string, expect authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error)
	// backendID is this instance's own SANDBOX_MANAGER_BACKEND_ID. Empty
	// only when capabilityVerify is also nil (an unconfigured instance).
	backendID string
}

// NewServer constructs a Server with the given stores. Space capability
// verification starts disabled; call WithCapabilityVerifier to enable it.
func NewServer(leases LeaseStore, snaps SnapshotStore) *Server {
	return &Server{leases: leases, snapshots: snaps, principal: authz.Principal}
}

// WithCapabilityVerifier wires Space capability-decision verification and
// this instance's own backend id into AcquireLease.
func (s *Server) WithCapabilityVerifier(verify func(token, claimsJSON string, expect authz.CapabilityExpectation) (authz.SpaceCapabilityClaims, error), backendID string) *Server {
	s.capabilityVerify = verify
	s.backendID = backendID
	return s
}

// AcquireLease creates a new lease for the requested scope.
func (s *Server) AcquireLease(ctx context.Context, req *AcquireLeaseRequest) (*AcquireLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "AcquireLease")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetScopeId() == "" {
		return nil, status.Error(codes.InvalidArgument, "scope_id is required")
	}
	if req.GetScopeType() == "" {
		return nil, status.Error(codes.InvalidArgument, "scope_type is required")
	}
	if req.GetScopeType() != "thread" && req.GetScopeType() != "agent" {
		return nil, status.Error(codes.InvalidArgument, "scope_type must be thread or agent")
	}
	if req.GetOrgId() == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id is required")
	}
	if req.GetOrgId() != principal.OrganizationID {
		return nil, status.Error(codes.PermissionDenied, "organization does not match verified identity")
	}
	ttl := time.Duration(0)
	if req.GetTtl() != nil {
		ttl = req.GetTtl().AsDuration()
	}
	if ttl <= 0 {
		return nil, status.Error(codes.InvalidArgument, "ttl must be greater than zero")
	}

	spaceID := strings.TrimSpace(req.GetSpaceId())
	backendID := ""
	if spaceID != "" {
		if s.capabilityVerify == nil {
			return nil, status.Error(codes.FailedPrecondition, "Space capability verification is not configured")
		}
		claims, err := s.capabilityVerify(req.GetCapabilityDecision(), req.GetCapabilityClaimsJson(), authz.CapabilityExpectation{
			OrgID: principal.OrganizationID, SpaceRef: spaceID, SubjectID: principal.ActorID, Now: time.Now().UTC(),
		})
		if err != nil {
			telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "capability_denied")))
			return nil, status.Error(codes.PermissionDenied, err.Error())
		}
		if claims.BackendID != s.backendID {
			telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "backend_mismatch")))
			return nil, mapErr(lease.ErrLeaseBackendMismatch)
		}
		backendID = claims.BackendID
	}

	l, err := s.leases.Create(ctx, req.GetScopeId(), req.GetScopeType(), principal.OrganizationID, principal.ActorID, spaceID, backendID, ttl)
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	return &AcquireLeaseResponse{
		LeaseId:   l.ID,
		Endpoint:  l.Endpoint,
		ExpiresAt: timestamppb.New(l.ExpiresAt),
		BackendId: l.BackendID,
		State:     l.State,
	}, nil
}

// ReleaseLease releases an existing lease.
func (s *Server) ReleaseLease(ctx context.Context, req *ReleaseLeaseRequest) (*ReleaseLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ReleaseLease")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	ok, err := s.leases.ReleaseScoped(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "released")))
	return &ReleaseLeaseResponse{Released: ok}, nil
}

// SnapshotSandbox persists a snapshot reference for the given lease.
func (s *Server) SnapshotSandbox(ctx context.Context, req *SnapshotRequest) (*SnapshotResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "SnapshotSandbox")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	if req.GetLabel() == "" {
		return nil, status.Error(codes.InvalidArgument, "label is required")
	}
	l, err := s.leases.BeginSnapshot(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", snapshotLeaseOutcome(err))))
		return nil, mapErr(err)
	}
	defer s.leases.EndSnapshot(ctx, l.ID)
	sn, err := s.snapshots.Create(ctx, l, req.GetLabel())
	if err != nil {
		telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", snapshotCreateOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	return &SnapshotResponse{
		SnapshotId: sn.ID,
		ObjectKey:  sn.ObjectKey,
	}, nil
}

// ActivateLease promotes a Space-scoped lease from SCRATCH to ACTIVE — the
// first time a caller needs more than the credential-free scratch
// allowlist (e.g. real backend network/egress permission per the
// capability decision's granted Permissions). A no-op returning the
// lease's current state if it is already ACTIVE or SNAPSHOTTING.
func (s *Server) ActivateLease(ctx context.Context, req *ActivateLeaseRequest) (*ActivateLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ActivateLease")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	l, err := s.leases.Activate(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "activated")))
	return &ActivateLeaseResponse{State: l.State}, nil
}

// Health reports serving status.
func (s *Server) Health(ctx context.Context, _ *SandboxHealthRequest) (*SandboxHealthResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "Health")))
	if _, err := s.principal(ctx); err != nil {
		return nil, err
	}
	return &SandboxHealthResponse{Status: "SERVING"}, nil
}

// Register wires the SandboxManager service onto the provided gRPC server.
func Register(g *grpc.Server, impl mpv1.SandboxManagerServer) {
	mpv1.RegisterSandboxManagerServer(g, impl)
}
