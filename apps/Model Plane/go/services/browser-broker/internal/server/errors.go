package server

import (
	"errors"

	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// ErrPolicyDenied signals that a browser-broker operation was rejected by
// policy. It is unified across all Model Plane gRPC services to map to
// [codes.PermissionDenied].
var ErrPolicyDenied = errors.New("policy denied")

// mapErr translates internal errors into gRPC status errors. It is the single
// source of truth for error → gRPC-code mappings in the browser-broker
// service. Unknown errors are reported as [codes.Internal]; a nil input
// yields nil.
func mapErr(err error) error {
	if err == nil {
		return nil
	}
	switch {
	case errors.Is(err, ErrPolicyDenied):
		return status.Error(codes.PermissionDenied, err.Error())
	case errors.Is(err, grant.ErrGrantNotFound):
		return status.Error(codes.NotFound, err.Error())
	case errors.Is(err, grant.ErrGrantRevoked), errors.Is(err, grant.ErrGrantExpired):
		return status.Error(codes.FailedPrecondition, err.Error())
	default:
		return status.Error(codes.Internal, err.Error())
	}
}

// grantOutcome classifies a grant-operation error for the grants-related
// telemetry counters.
func grantOutcome(err error) string {
	switch {
	case err == nil:
		return "ok"
	case errors.Is(err, ErrPolicyDenied):
		return "policy_denied"
	case errors.Is(err, grant.ErrGrantNotFound):
		return "not_found"
	case errors.Is(err, grant.ErrGrantRevoked):
		return "revoked"
	case errors.Is(err, grant.ErrGrantExpired):
		return "expired"
	default:
		return "internal_error"
	}
}
