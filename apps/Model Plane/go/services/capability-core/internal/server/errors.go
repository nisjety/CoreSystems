package server

import (
	"errors"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// mapErr translates internal domain errors into gRPC status errors.
//
// This is the single source of truth for error → gRPC-code mappings in the
// capability-core service. In particular, [domain.ErrPolicyDenied] is unified
// to [codes.PermissionDenied] across all Model Plane gRPC services.
//
// Unknown errors are reported as [codes.Internal]. A nil input yields nil.
func mapErr(err error) error {
	if err == nil {
		return nil
	}
	switch {
	case errors.Is(err, domain.ErrInvalidArgument):
		return status.Error(codes.InvalidArgument, err.Error())
	case errors.Is(err, domain.ErrCapabilityNotFound):
		return status.Error(codes.NotFound, err.Error())
	case errors.Is(err, domain.ErrVersionMismatch):
		return status.Error(codes.FailedPrecondition, err.Error())
	case errors.Is(err, domain.ErrPolicyDenied):
		return status.Error(codes.PermissionDenied, err.Error())
	default:
		return status.Error(codes.Internal, err.Error())
	}
}
