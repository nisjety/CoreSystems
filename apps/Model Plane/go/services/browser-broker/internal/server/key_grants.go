package server

import (
	"context"
	"errors"
	"fmt"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// INNH-10: grants for an api_key principal (an API key exchanged by Auth Core
// for a short-lived token). Everything here is dark behind
// BROWSER_BROKER_KEY_GRANTS_ENABLED.

// errKeyGrantsDisabled refuses every api_key call while the switch is off.
var errKeyGrantsDisabled = fmt.Errorf("%w: api key browser grants are disabled", ErrPolicyDenied)

// acquireKeyGrant issues a key grant. The key grant itself is MP-2; until it
// lands every api_key acquire is refused, switch on or off.
func (s *Server) acquireKeyGrant(ctx context.Context, req *AcquireGrantRequest, _ string, principal authctx.Principal) (*AcquireGrantResponse, error) {
	if !s.cfg.KeyGrantsEnabled {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "key_grants_disabled")))
		return nil, mapErr(errKeyGrantsDisabled)
	}
	if err := authz.APIKeyAcquireShape(req); err != nil {
		return nil, status.Error(codes.PermissionDenied, err.Error())
	}
	if principal.KeyID == "" || !principal.HasScope(authz.ScopeAgent) {
		return nil, status.Error(codes.PermissionDenied, "api key principal lacks browser:agent")
	}
	return nil, status.Error(codes.PermissionDenied, "api key browser grants are not available")
}

// keyMayRevoke lets an api_key revoke only a live grant stamped with its own
// key (inside its org and owner). Any other grant, and a grant that is already
// revoked or expired, is NotFound: a key learns nothing about grants that are
// not its own, and a repeated revoke of its own reads as "already gone" (the
// model-gateway door maps that to an idempotent delete). Other principal types
// pass unchanged: the owner filter applies to them in RevokeScoped.
func (s *Server) keyMayRevoke(ctx context.Context, principal authctx.Principal, grantID string) error {
	if principal.PrincipalType != authz.PrincipalAPIKey {
		return nil
	}
	if !s.cfg.KeyGrantsEnabled {
		return errKeyGrantsDisabled
	}
	g, err := s.grants.GetScoped(ctx, grantID, principal.OrganizationID, authz.OwnerFilter(principal))
	if errors.Is(err, grant.ErrGrantRevoked) || errors.Is(err, grant.ErrGrantExpired) {
		return grant.ErrGrantNotFound
	}
	if err != nil {
		return err
	}
	if !authz.KeyMatches(principal, g.KeyID) {
		return grant.ErrGrantNotFound
	}
	return nil
}

// keyGrantLive applies the key rules to a grant a caller is validating: an
// api_key caller sees only its own key's grants (NotFound otherwise), and a
// key grant validates as revoked while the switch is off, so turning it off
// stops every key-driven run at its next step.
func (s *Server) keyGrantLive(principal authctx.Principal, g *grant.Grant) error {
	if !authz.KeyMatches(principal, g.KeyID) {
		return grant.ErrGrantNotFound
	}
	if g.KeyID != "" && !s.cfg.KeyGrantsEnabled {
		return grant.ErrGrantRevoked
	}
	return nil
}
