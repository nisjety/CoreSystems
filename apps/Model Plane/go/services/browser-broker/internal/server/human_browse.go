package server

import (
	"context"
	"strings"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// Grant kinds a caller may ask AcquireGrant for (WS8.C). The empty string is
// the ordinary session or sensitive-approval path. Eligibility for
// human_browse is decided here and by the policy, never by the caller.
const grantKindHumanBrowse = "human_browse"

// derivePurposeAttach derives an attach grant bound to one run (WS8.C).
const derivePurposeAttach = grant.PurposeAttach

// acquireHumanBrowse issues a human_browse grant: open web minus the policy's
// deny-list, for the person who asks. It refuses anything that would turn it
// into a different grant (a host list, sensitive scopes, a budget claim, a
// parent) instead of ignoring the extra field.
func (s *Server) acquireHumanBrowse(ctx context.Context, req *AcquireGrantRequest, scopeURL string, principal authctx.Principal) (*AcquireGrantResponse, error) {
	orgID, actorID := principal.OrganizationID, principal.ActorID
	// A service principal never mints a person's browser grant.
	if principal.PrincipalType != "user" {
		return nil, status.Error(codes.PermissionDenied, "a human_browse grant is issued to a signed-in user only")
	}
	if len(req.GetAllowedDomains()) != 0 {
		return nil, status.Error(codes.InvalidArgument, "a human_browse grant takes no allowed_domains")
	}
	if len(req.GetAllowedActions()) != 0 || len(req.GetAllowedFrameIds()) != 0 || len(req.GetAllowedDialogIds()) != 0 ||
		len(req.GetAllowedArtifactIds()) != 0 || req.GetParentGrantId() != "" {
		return nil, status.Error(codes.InvalidArgument, "a human_browse grant cannot carry sensitive scopes or a parent")
	}
	if req.GetBudgetMicroUsd() != 0 || req.GetBudgetMaxSteps() != 0 || req.GetBudgetMaxRuntimeS() != 0 ||
		req.GetBudgetSource() != "" || req.GetBudgetMintedAtUnix() != 0 {
		return nil, status.Error(codes.InvalidArgument, "a human_browse grant cannot carry a budget claim")
	}
	pol, err := s.cfg.Policy.ResolveHumanBrowse(ctx, orgID, actorID)
	if err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	if !pol.Allowed {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "human_browse_disabled")))
		return nil, mapErr(grant.ErrHumanBrowseDisabled)
	}
	zdr := req.GetZdr() || principal.ZeroDataRetention
	g, err := s.grants.CreateHumanBrowse(ctx, grant.CreateHumanBrowseParams{
		OrgID:             orgID,
		OwnerID:           actorID,
		SessionKey:        req.GetSessionKey(),
		ScopeURL:          scopeURL,
		DeniedDomains:     pol.DeniedSuffixes,
		TTL:               s.cfg.TTL,
		AbsoluteTTL:       s.cfg.HumanAbsoluteTTL,
		Zdr:               zdr,
		MaxActivePerOwner: s.cfg.MaxActivePerOwner,
	})
	if err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	logGrantIssued(ctx, g)
	return s.acquireResponse(g), nil
}

// acquireResponse is the AcquireGrant answer for any kind.
func (s *Server) acquireResponse(g *grant.Grant) *AcquireGrantResponse {
	return &AcquireGrantResponse{
		GrantId:            g.ID,
		Endpoint:           s.baseURL + "/grants/" + g.ID,
		ExpiresAt:          timestamppb.New(g.ExpiresAt),
		AllowedDomains:     append([]string(nil), g.AllowedDomains...),
		AllowedActions:     append([]string(nil), g.AllowedActions...),
		AllowedFrameIds:    append([]string(nil), g.AllowedFrameIDs...),
		AllowedDialogIds:   append([]string(nil), g.AllowedDialogIDs...),
		AllowedArtifactIds: append([]string(nil), g.AllowedArtifactIDs...),
		ParentGrantId:      g.ParentGrantID,
		Kind:               string(g.Kind),
		Zdr:                g.Zdr,
		BudgetMicroUsd:     g.Budget.MicroUSD,
		BudgetMaxSteps:     g.Budget.MaxSteps,
		BudgetMaxRuntimeS:  g.Budget.MaxRuntimeS,
		BudgetSource:       g.Budget.Source,
		BudgetMintedAtUnix: g.Budget.MintedAtUnix,
		PolicyMode:         policyModeOf(g),
		DeniedDomains:      append([]string(nil), g.DeniedDomains...),
		AbsoluteExpiresAt:  timestamppb.New(absoluteOf(g)),
		KeyId:              g.KeyID,
		ConsentScope:       g.ConsentScope,
	}
}

// policyModeOf names a grant's navigation policy; a grant stored before
// WS8.C has none and is an allowlist.
func policyModeOf(g *grant.Grant) string {
	if g.PolicyMode == "" {
		return grant.PolicyModeAllowlist
	}
	return g.PolicyMode
}

// absoluteOf is a grant's hard backstop; a grant without one expires at its
// own expiry.
func absoluteOf(g *grant.Grant) time.Time {
	if g.AbsoluteExpiresAt.IsZero() {
		return g.ExpiresAt
	}
	return g.AbsoluteExpiresAt
}

// RenewGrant extends a session or human_browse grant by one TTL, never past
// its absolute expiry. Only the grant's own user may renew it: a service
// principal is refused even with browser:write, so no workload can keep a
// person's browser session alive. A grant at its cap, an expired or revoked
// one and any other kind answer FailedPrecondition; another user's grant is
// NotFound.
func (s *Server) RenewGrant(ctx context.Context, req *RenewGrantRequest) (*RenewGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "RenewGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if principal.PrincipalType != "user" {
		return nil, status.Error(codes.PermissionDenied, "a grant is renewed by its user only")
	}
	if strings.TrimSpace(req.GetGrantId()) == "" {
		return nil, status.Error(codes.InvalidArgument, "grant_id is required")
	}
	if req.GetOrgId() == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id is required")
	}
	if req.GetOrgId() != principal.OrganizationID {
		return nil, status.Error(codes.PermissionDenied, "organization does not match verified identity")
	}
	g, err := s.grants.Renew(ctx, grant.RenewParams{
		OrgID:   principal.OrganizationID,
		OwnerID: principal.ActorID,
		GrantID: req.GetGrantId(),
		TTL:     s.cfg.TTL,
	})
	if err != nil {
		telemetry.GrantsRenewedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsRenewedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "renewed")))
	return &RenewGrantResponse{
		ExpiresAt:         timestamppb.New(g.ExpiresAt),
		AbsoluteExpiresAt: timestamppb.New(absoluteOf(g)),
	}, nil
}
