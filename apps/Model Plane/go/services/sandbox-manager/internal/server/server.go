package server

import (
	"context"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// Server implements SandboxManagerServer backed by in-memory lease and
// snapshot stores.
type Server struct {
	mpv1.UnimplementedSandboxManagerServer
	leases    *lease.Store
	snapshots *snapshot.Store
}

// NewServer constructs a Server with the given stores.
func NewServer(leases *lease.Store, snaps *snapshot.Store) *Server {
	return &Server{leases: leases, snapshots: snaps}
}

// AcquireLease creates a new lease for the requested scope.
func (s *Server) AcquireLease(ctx context.Context, req *AcquireLeaseRequest) (*AcquireLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "AcquireLease")))
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
	ttl := time.Duration(0)
	if req.GetTtl() != nil {
		ttl = req.GetTtl().AsDuration()
	}
	if ttl <= 0 {
		return nil, status.Error(codes.InvalidArgument, "ttl must be greater than zero")
	}
	l, err := s.leases.Create(req.GetScopeId(), req.GetScopeType(), req.GetOrgId(), ttl)
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	return &AcquireLeaseResponse{
		LeaseId:   l.ID,
		Endpoint:  l.Endpoint,
		ExpiresAt: timestamppb.New(l.ExpiresAt),
	}, nil
}

// ReleaseLease releases an existing lease.
func (s *Server) ReleaseLease(ctx context.Context, req *ReleaseLeaseRequest) (*ReleaseLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ReleaseLease")))
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	ok, err := s.leases.Release(req.GetLeaseId())
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
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	if req.GetLabel() == "" {
		return nil, status.Error(codes.InvalidArgument, "label is required")
	}
	l, err := s.leases.Get(req.GetLeaseId())
	if err != nil {
		telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", snapshotLeaseOutcome(err))))
		return nil, mapErr(err)
	}
	sn, err := s.snapshots.Create(l, req.GetLabel())
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

// Health reports serving status.
func (s *Server) Health(ctx context.Context, _ *SandboxHealthRequest) (*SandboxHealthResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "Health")))
	return &SandboxHealthResponse{Status: "SERVING"}, nil
}

// Register wires the SandboxManager service onto the provided gRPC server.
func Register(g *grpc.Server, impl mpv1.SandboxManagerServer) {
	mpv1.RegisterSandboxManagerServer(g, impl)
}
