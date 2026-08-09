package server

import (
	"errors"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// ErrPolicyDenied signals that a letta-bridge operation was rejected by
// policy. It is unified across all Model Plane gRPC services to map to
// [codes.PermissionDenied].
var ErrPolicyDenied = errors.New("policy denied")

// ErrInvalidArgument signals that a request failed validation in the
// memstore layer. It is classified as [codes.InvalidArgument] by [mapErr].
//
// A dedicated sentinel lets handlers and tests distinguish validation
// failures from truly unexpected errors.
var ErrInvalidArgument = errors.New("invalid argument")

// mapErr translates internal errors into gRPC status errors. It is the single
// source of truth for error → gRPC-code mappings in the letta-bridge
// service. Unknown errors are reported as [codes.Internal]; a nil input
// yields nil.
func mapErr(err error) error {
	if err == nil {
		return nil
	}
	switch {
	case errors.Is(err, ErrPolicyDenied):
		return status.Error(codes.PermissionDenied, err.Error())
	case errors.Is(err, ErrInvalidArgument):
		return status.Error(codes.InvalidArgument, err.Error())
	default:
		return status.Error(codes.Internal, err.Error())
	}
}

// indexOutcome classifies an IndexMemory error for the
// telemetry.MemoryIndexedTotal counter.
func indexOutcome(err error) string {
	switch {
	case err == nil:
		return "ok"
	case errors.Is(err, ErrPolicyDenied):
		return "policy_denied"
	case errors.Is(err, ErrInvalidArgument):
		return "invalid_argument"
	default:
		return "internal_error"
	}
}

// deleteOutcome classifies a successful DeleteMemory call (no backend error)
// for the telemetry.MemoryDeletedTotal counter, distinguishing an actual
// removal from a well-formed "there was nothing to delete" response.
func deleteOutcome(deleted bool) string {
	if deleted {
		return "deleted"
	}
	return "not_found"
}
