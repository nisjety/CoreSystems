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
	case errors.Is(err, grant.ErrInvalidDomainPolicy), errors.Is(err, grant.ErrDomainNotGrantable),
		errors.Is(err, grant.ErrDeriveNeedsExplicitDomains):
		return status.Error(codes.InvalidArgument, err.Error())
	case errors.Is(err, grant.ErrTooManyGrants):
		return status.Error(codes.ResourceExhausted, err.Error())
	case errors.Is(err, grant.ErrDeriveWidensParent), errors.Is(err, grant.ErrDeriveWidensBudget):
		return status.Error(codes.PermissionDenied, err.Error())
	case errors.Is(err, grant.ErrInvalidBudget):
		return status.Error(codes.InvalidArgument, err.Error())
	case errors.Is(err, grant.ErrParentNotDerivable), errors.Is(err, grant.ErrApprovalParentInvalid),
		errors.Is(err, grant.ErrZdrPostureChanged):
		return status.Error(codes.FailedPrecondition, err.Error())
	case errors.Is(err, grant.ErrDeriveConflict):
		return status.Error(codes.AlreadyExists, err.Error())
	case errors.Is(err, grant.ErrInvalidGrantText):
		return status.Error(codes.InvalidArgument, grant.ErrInvalidGrantText.Error())
	case errors.Is(err, grant.ErrStoreUnavailable):
		// The durable store failed: retryable, never a fail-open.
		return status.Error(codes.Unavailable, grant.ErrStoreUnavailable.Error())
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
	case errors.Is(err, grant.ErrInvalidDomainPolicy), errors.Is(err, grant.ErrDomainNotGrantable):
		return "domain_not_grantable"
	case errors.Is(err, grant.ErrTooManyGrants):
		return "too_many_grants"
	case errors.Is(err, grant.ErrDeriveWidensParent):
		return "derive_widens_parent"
	case errors.Is(err, grant.ErrDeriveWidensBudget):
		return "derive_widens_budget"
	case errors.Is(err, grant.ErrInvalidBudget):
		return "invalid_budget"
	case errors.Is(err, grant.ErrDeriveNeedsExplicitDomains):
		return "derive_needs_explicit_domains"
	case errors.Is(err, grant.ErrParentNotDerivable), errors.Is(err, grant.ErrApprovalParentInvalid):
		return "parent_not_derivable"
	case errors.Is(err, grant.ErrZdrPostureChanged):
		return "zdr_posture_changed"
	case errors.Is(err, grant.ErrDeriveConflict):
		return "derive_conflict"
	case errors.Is(err, grant.ErrInvalidGrantText):
		return "invalid_input"
	case errors.Is(err, grant.ErrStoreUnavailable):
		return "store_unavailable"
	default:
		return "internal_error"
	}
}
