package server

import (
	"errors"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// ErrPolicyDenied signals that a sandbox-manager operation was rejected by
// policy. It is unified across all Model Plane gRPC services to map to
// [codes.PermissionDenied].
var ErrPolicyDenied = errors.New("policy denied")

// mapErr translates internal errors into gRPC status errors. It is the single
// source of truth for error → gRPC-code mappings in the sandbox-manager
// service. Unknown errors are reported as [codes.Internal]; a nil input
// yields nil.
func mapErr(err error) error {
	if err == nil {
		return nil
	}
	switch {
	case errors.Is(err, ErrPolicyDenied):
		return status.Error(codes.PermissionDenied, err.Error())
	case errors.Is(err, lease.ErrLeaseNotFound):
		return status.Error(codes.NotFound, err.Error())
	case errors.Is(err, lease.ErrLeaseExpired):
		return status.Error(codes.FailedPrecondition, err.Error())
	case errors.Is(err, snapshot.ErrInvalidLease):
		return status.Error(codes.FailedPrecondition, err.Error())
	default:
		return status.Error(codes.Internal, err.Error())
	}
}

// leaseOutcome classifies a lease-operation error for the
// telemetry.LeaseDecisionsTotal counter.
func leaseOutcome(err error) string {
	switch {
	case err == nil:
		return "ok"
	case errors.Is(err, ErrPolicyDenied):
		return "policy_denied"
	case errors.Is(err, lease.ErrLeaseNotFound):
		return "not_found"
	case errors.Is(err, lease.ErrLeaseExpired):
		return "expired"
	default:
		return "internal_error"
	}
}

// snapshotLeaseOutcome classifies an error from the lease-lookup stage of
// SnapshotSandbox for the telemetry.SnapshotDecisionsTotal counter.
func snapshotLeaseOutcome(err error) string {
	switch {
	case err == nil:
		return "ok"
	case errors.Is(err, ErrPolicyDenied):
		return "policy_denied"
	case errors.Is(err, lease.ErrLeaseNotFound):
		return "lease_not_found"
	case errors.Is(err, lease.ErrLeaseExpired):
		return "lease_expired"
	default:
		return "lease_internal_error"
	}
}

// snapshotCreateOutcome classifies an error from the snapshot-creation stage
// of SnapshotSandbox for the telemetry.SnapshotDecisionsTotal counter.
func snapshotCreateOutcome(err error) string {
	switch {
	case err == nil:
		return "ok"
	case errors.Is(err, ErrPolicyDenied):
		return "policy_denied"
	case errors.Is(err, snapshot.ErrInvalidLease):
		return "invalid_lease"
	default:
		return "snapshot_internal_error"
	}
}
