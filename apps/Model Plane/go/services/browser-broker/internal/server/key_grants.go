package server

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// INNH-10: grants for an api_key principal (an API key exchanged by Auth Core
// for a short-lived token). Everything here is dark behind
// BROWSER_BROKER_KEY_GRANTS_ENABLED.

// errKeyGrantsDisabled refuses every api_key call while the switch is off.
var errKeyGrantsDisabled = fmt.Errorf("%w: api key browser grants are disabled", ErrPolicyDenied)

// ErrorInfo reasons of the key refusals, so the model-gateway door (MP-3) can
// tell them apart without parsing a message.
const (
	reasonKeyGrantsDisabled = "API_KEY_GRANTS_DISABLED"
	reasonKeyRevoked        = "API_KEY_REVOKED"
	reasonKeyHostCeiling    = "API_KEY_HOST_CEILING"
	reasonKeyBudgetCeiling  = "API_KEY_BUDGET_CEILING"
)

// keyRefusal is a gRPC error with an ErrorInfo reason and no metadata: no
// host and no value the key asked for rides along.
func keyRefusal(code codes.Code, reason, msg string) error {
	st := status.New(code, msg)
	if withDetails, err := st.WithDetails(&errdetails.ErrorInfo{Reason: reason}); err == nil {
		return withDetails.Err()
	}
	return st.Err()
}

func keyOutcome(ctx context.Context, outcome string) {
	telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", outcome)))
}

// acquireKeyGrant issues a key grant (INNH-10 MP-2): an ordinary allow-list
// session grant stamped with the key, whose hosts are a subset of the key's
// host ceiling, whose budget is mandatory and within the key's budget
// ceiling, and whose lifetime is the key-grant TTL (a request may only
// shorten it; it is never renewed). It counts against the per-key cap and the
// owner's cap. ZDR follows the ordinary floor, so a ZDR key grant lives in
// memory with a z_ id.
func (s *Server) acquireKeyGrant(ctx context.Context, req *AcquireGrantRequest, scopeURL string, principal authctx.Principal) (*AcquireGrantResponse, error) {
	if !s.keyGrantsOn() {
		keyOutcome(ctx, "key_grants_disabled")
		return nil, keyRefusal(codes.PermissionDenied, reasonKeyGrantsDisabled, errKeyGrantsDisabled.Error())
	}
	if err := authz.APIKeyAcquireShape(req); err != nil {
		return nil, status.Error(codes.PermissionDenied, err.Error())
	}
	if !grant.ValidKeyID(principal.KeyID) || !principal.HasScope(authz.ScopeAgent) {
		return nil, status.Error(codes.PermissionDenied, "api key principal lacks browser:agent")
	}
	if s.revokedKeys.revoked(principal.KeyID) {
		keyOutcome(ctx, "key_revoked")
		return nil, keyRefusal(codes.PermissionDenied, reasonKeyRevoked, "api key is revoked")
	}
	domains, err := grant.NormalizeAllowedDomains(req.GetAllowedDomains())
	if err != nil {
		return nil, status.Error(codes.InvalidArgument, "allowed_domains must contain 1 to 32 bounded hostnames")
	}
	if !withinHostCeiling(domains, principal.KeyHostCeiling) {
		keyOutcome(ctx, "key_host_ceiling")
		return nil, keyRefusal(codes.PermissionDenied, reasonKeyHostCeiling, "allowed_domains are outside the api key's host ceiling")
	}
	// WS7.K: the org web policy narrows the requested hosts as on the ordinary
	// path. An allowed host may be a subdomain of a requested one, which the
	// exact-host ceiling refuses, so the hosts that are granted are checked too.
	webPolicy, domains, err := s.applyWebPolicy(ctx, principal, domains)
	if err != nil {
		return nil, err
	}
	if !withinHostCeiling(domains, principal.KeyHostCeiling) {
		keyOutcome(ctx, "key_host_ceiling")
		return nil, keyRefusal(codes.PermissionDenied, reasonKeyHostCeiling, "allowed_domains are outside the api key's host ceiling")
	}
	if err := s.cfg.Policy.CheckSession(ctx, principal.OrganizationID, domains); err != nil {
		keyOutcome(ctx, grantOutcome(err))
		return nil, mapErr(err)
	}
	budget, err := budgetFromRequest(
		req.GetBudgetMicroUsd(), req.GetBudgetMaxSteps(), req.GetBudgetMaxRuntimeS(),
		req.GetBudgetSource(), req.GetBudgetMintedAtUnix(),
	)
	if err != nil {
		return nil, err
	}
	if budget.MicroUSD <= 0 || budget.MaxSteps <= 0 || budget.MaxRuntimeS <= 0 {
		return nil, status.Error(codes.InvalidArgument,
			"an api key grant needs a budget: budget_micro_usd, budget_max_steps and budget_max_runtime_s must all be positive")
	}
	if !withinBudgetCeiling(budget, principal.KeyBudgetCeiling) {
		keyOutcome(ctx, "key_budget_ceiling")
		return nil, keyRefusal(codes.PermissionDenied, reasonKeyBudgetCeiling, "budget is over the api key's budget ceiling")
	}
	ttl, err := s.keyGrantTTL(req.GetTtlSeconds())
	if err != nil {
		return nil, err
	}
	// ZDR is a floor, as on the ordinary path.
	zdr := req.GetZdr() || principal.ZeroDataRetention
	readOnly := req.GetReadOnly()
	if readOnly && !zdr {
		return nil, status.Error(codes.FailedPrecondition,
			"a read-only browser grant must be Zero Data Retention until the durable store persists it")
	}
	domains, denied, err := s.applySitePolicy(ctx, principal, domains, readOnly)
	if err != nil {
		return nil, err
	}
	g, err := s.grants.CreateSession(ctx, grant.CreateSessionParams{
		OrgID:             principal.OrganizationID,
		OwnerID:           principal.ActorID,
		SessionKey:        req.GetSessionKey(),
		ScopeURL:          scopeURL,
		AllowedDomains:    domains,
		TTL:               ttl,
		Zdr:               zdr,
		ReadOnly:          readOnly,
		Budget:            budget,
		WebPolicy:         webPolicy,
		MaxActivePerOwner: s.cfg.MaxActivePerOwner,
		KeyID:             principal.KeyID,
		MaxActivePerKey:   s.cfg.MaxActivePerKey,
	})
	if err != nil {
		keyOutcome(ctx, grantOutcome(err))
		return nil, mapErr(err)
	}
	// A revoke that landed while this grant was written: RevokeKey marks the
	// list before it sweeps, so either the sweep saw this grant or this check
	// sees the mark. Either way the grant does not survive (and keyGrantLive
	// refuses it even if this revoke fails).
	if s.revokedKeys.revoked(principal.KeyID) {
		_ = s.grants.RevokeScoped(ctx, g.ID, g.OrgID, g.OwnerID, grant.KeyRevokedReason)
		keyOutcome(ctx, "key_revoked")
		return nil, keyRefusal(codes.PermissionDenied, reasonKeyRevoked, "api key is revoked")
	}
	keyOutcome(ctx, "created")
	logGrantIssued(ctx, g)
	resp := s.acquireResponse(g)
	resp.ReadOnly = g.ReadOnly
	resp.WebPolicy = s.webPolicyResponse(g)
	resp.DeniedDomains = append(resp.DeniedDomains, denied...)
	return resp, nil
}

// withinHostCeiling: every requested host is exactly a host of the key's
// ceiling (no subdomain widening, OD-6). A ceiling that does not normalize
// (empty, malformed, over 32 hosts) admits nothing.
func withinHostCeiling(domains, ceiling []string) bool {
	normalized, err := grant.NormalizeAllowedDomains(ceiling)
	if err != nil {
		return false
	}
	allowed := make(map[string]struct{}, len(normalized))
	for _, host := range normalized {
		allowed[host] = struct{}{}
	}
	for _, host := range domains {
		if _, ok := allowed[host]; !ok {
			return false
		}
	}
	return true
}

// withinBudgetCeiling: every dimension is within the key's ceiling. A ceiling
// with a zero (absent) dimension admits nothing: a key without a full budget
// ceiling cannot drive the browser.
func withinBudgetCeiling(b grant.Budget, ceiling authctx.KeyBudget) bool {
	if ceiling.MicroUSD <= 0 || ceiling.MaxSteps <= 0 || ceiling.MaxRuntimeS <= 0 {
		return false
	}
	return b.MicroUSD <= ceiling.MicroUSD && b.MaxSteps <= ceiling.MaxSteps && b.MaxRuntimeS <= ceiling.MaxRuntimeS
}

// keyGrantTTL resolves a key grant's lifetime: 0 is KeyGrantTTL, a value
// under MinKeyGrantTTL is refused, a longer one is cut to KeyGrantTTL.
func (s *Server) keyGrantTTL(seconds int32) (time.Duration, error) {
	limit := clampDuration(s.cfg.KeyGrantTTL, MinKeyGrantTTL, MaxKeyGrantTTL)
	if seconds == 0 {
		return limit, nil
	}
	if seconds < 0 || time.Duration(seconds)*time.Second < MinKeyGrantTTL {
		return 0, status.Error(codes.InvalidArgument, "ttl_seconds is below the minimum key grant lifetime")
	}
	return min(time.Duration(seconds)*time.Second, limit), nil
}

// RevokeKey ends every grant of an API key (INNH-10, the api_key_revoked
// event). The key is marked revoked first, so an acquire racing the sweep
// refuses itself; then every live grant of the key is revoked in both stores
// with reason key_revoked. It returns how many grants it revoked. A store
// error is returned so the event is redelivered; the mark already stands.
// It runs whatever the switch says: turning key grants off must never stop a
// revoke.
func (s *Server) RevokeKey(ctx context.Context, keyID string) (int64, error) {
	if !grant.ValidKeyID(keyID) {
		return 0, grant.ErrInvalidKeyID
	}
	s.revokedKeys.mark(keyID)
	revoker, ok := s.grants.(grant.KeyRevoker)
	if !ok {
		return 0, errors.New("grant store cannot revoke by api key")
	}
	n, err := revoker.RevokeByKey(ctx, keyID, grant.KeyRevokedReason)
	if n > 0 {
		telemetry.GrantsRevokedTotal.Add(ctx, n, metric.WithAttributes(attribute.String("outcome", "key_revoked")))
	}
	return n, err
}

// keyGrantsOn is the switch: configured on and not disabled since
// (DisableKeyGrants).
func (s *Server) keyGrantsOn() bool {
	return s.cfg.KeyGrantsEnabled && !s.keyGrantsDisabled.Load()
}

// DisableKeyGrants turns key grants off for the life of this process, as if
// the switch were off: every api_key acquire and revoke is refused and every
// key grant validates as revoked. The broker main calls it when the
// api_key_revoked consumer cannot bind, so a key grant never outlives its key
// unnoticed. There is no way back on short of a restart.
func (s *Server) DisableKeyGrants() { s.keyGrantsDisabled.Store(true) }

// KeyGrantsActive reports whether this process issues key grants.
func (s *Server) KeyGrantsActive() bool { return s.keyGrantsOn() }

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
	if !s.keyGrantsOn() {
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
// stops every key-driven run at its next step. A grant of a key on the
// revoked list validates as revoked too, even if the sweep missed it.
func (s *Server) keyGrantLive(principal authctx.Principal, g *grant.Grant) error {
	if !authz.KeyMatches(principal, g.KeyID) {
		return grant.ErrGrantNotFound
	}
	if g.KeyID != "" && (!s.keyGrantsOn() || s.revokedKeys.revoked(g.KeyID)) {
		return grant.ErrGrantRevoked
	}
	return nil
}
