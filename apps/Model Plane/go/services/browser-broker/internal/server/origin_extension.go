package server

import (
	"context"
	"errors"
	"fmt"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// E3.2-e1 (OD13): origin-extension grants. A signed-in person consents to one
// more origin for a run; the broker records it as a parent-bound grant that
// Quarry may widen its egress policy from, only while it validates. Dark
// behind BROWSER_BROKER_ORIGIN_EXTENSION_ENABLED.

// grantKindOriginExtension is the grant_kind of an origin extension.
const grantKindOriginExtension = authz.GrantKindOriginExtension

// errOriginExtensionDisabled refuses every extension request while the switch
// is off.
var errOriginExtensionDisabled = fmt.Errorf("%w: origin extension grants are disabled", ErrPolicyDenied)

// ErrorInfo reasons of the extension refusals. No host rides along.
const (
	reasonOriginExtensionDisabled = "ORIGIN_EXTENSION_DISABLED"
	reasonOriginExtensionDenied   = "ORIGIN_EXTENSION_DENIED"
)

// acquireOriginExtension issues an origin-extension grant. Only a signed-in
// user asks (a person's consent is not a workload's to give), the parent is a
// live session or run grant of that user, and the request carries nothing
// that would turn it into another grant: no sensitive scope, no budget claim,
// no read-only flag (it inherits the parent's level).
//
// Deny beats approval at every layer, in this order: the consent's own denied
// entries (grant.ApplyConsentDenies), the broker's static policy (internal
// namespaces), and the org site policy (an "ask" host is allowed here, since
// the person just answered the question; "deny" and unanswered hosts are not).
func (s *Server) acquireOriginExtension(ctx context.Context, req *AcquireGrantRequest, scopeURL string, principal authctx.Principal) (*AcquireGrantResponse, error) {
	if !s.cfg.OriginExtensionEnabled {
		keyOutcome(ctx, "origin_extension_disabled")
		return nil, keyRefusal(codes.PermissionDenied, reasonOriginExtensionDisabled, errOriginExtensionDisabled.Error())
	}
	if principal.PrincipalType != authz.PrincipalUser {
		return nil, status.Error(codes.PermissionDenied, "an origin extension is issued to a signed-in user only")
	}
	if req.GetParentGrantId() == "" {
		return nil, status.Error(codes.InvalidArgument, "an origin extension requires parent_grant_id")
	}
	if len(req.GetAllowedActions()) != 0 || len(req.GetAllowedFrameIds()) != 0 || len(req.GetAllowedDialogIds()) != 0 ||
		len(req.GetAllowedArtifactIds()) != 0 {
		return nil, status.Error(codes.InvalidArgument, "an origin extension cannot carry sensitive scopes")
	}
	if req.GetBudgetMicroUsd() != 0 || req.GetBudgetMaxSteps() != 0 || req.GetBudgetMaxRuntimeS() != 0 ||
		req.GetBudgetSource() != "" || req.GetBudgetMintedAtUnix() != 0 {
		return nil, status.Error(codes.InvalidArgument, "an origin extension cannot carry a budget claim")
	}
	if req.GetReadOnly() {
		return nil, status.Error(codes.InvalidArgument, "an origin extension takes its parent's access level and cannot be read-only on its own")
	}
	scope := req.GetConsentScope()
	zdr := req.GetZdr() || principal.ZeroDataRetention
	if err := grant.CheckConsentScope(scope, zdr); err != nil {
		return nil, status.Error(codes.InvalidArgument, err.Error())
	}
	hosts, err := grant.NormalizeExtensionHosts(req.GetAllowedDomains())
	if err != nil {
		if errors.Is(err, grant.ErrWildcardRefused) {
			return nil, status.Error(codes.InvalidArgument, err.Error())
		}
		return nil, status.Error(codes.InvalidArgument, "allowed_domains must contain 1 to 8 exact hostnames")
	}
	var denied []string
	if len(req.GetConsentDeniedDomains()) != 0 {
		denied, err = grant.NormalizeDeniedDomains(req.GetConsentDeniedDomains())
		if err != nil {
			return nil, status.Error(codes.InvalidArgument, "consent_denied_domains must be bounded hostnames")
		}
	}
	if err := s.cfg.Policy.CheckSession(ctx, principal.OrganizationID, hosts); err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	kept, dropped := grant.ApplyConsentDenies(hosts, denied)
	if len(kept) == 0 {
		keyOutcome(ctx, "extension_denied")
		return nil, keyRefusal(codes.PermissionDenied, reasonOriginExtensionDenied, grant.ErrExtensionDenied.Error())
	}
	kept, siteDropped, err := s.applyExtensionSitePolicy(ctx, principal, kept)
	if err != nil {
		return nil, err
	}
	ttl, err := s.requestTTL(req.GetTtlSeconds())
	if err != nil {
		return nil, err
	}
	g, err := s.grants.CreateOriginExtension(ctx, grant.ExtensionParams{
		OrgID:          principal.OrganizationID,
		OwnerID:        principal.ActorID,
		SessionKey:     req.GetSessionKey(),
		ScopeURL:       scopeURL,
		ParentGrantID:  req.GetParentGrantId(),
		AllowedDomains: kept,
		ConsentScope:   scope,
		TTL:            ttl,
		Zdr:            zdr,
	})
	if err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	logGrantIssued(ctx, g)
	resp := s.acquireResponse(g)
	resp.ReadOnly = g.ReadOnly
	// The parent's snapshot, copied unchanged by the grant layer (WS7.K).
	resp.WebPolicy = s.webPolicyResponse(g)
	// The hosts the person's own denies and the org policy dropped are the hosts
	// this extension may never add.
	resp.DeniedDomains = append(resp.DeniedDomains, reportedDenied(append(dropped, siteDropped...))...)
	return resp, nil
}

// extensionLive applies the switch to a grant a caller is validating: an
// origin extension validates as revoked while the switch is off, so turning it
// off ends every widened egress policy at the consumer's next revalidation.
func (s *Server) extensionLive(g *grant.Grant) error {
	if g.Kind == grant.KindOriginExtension && !s.cfg.OriginExtensionEnabled {
		return grant.ErrGrantRevoked
	}
	return nil
}

// applyExtensionSitePolicy narrows an extension's hosts to what org-core allows
// for this caller. It mirrors applySitePolicy (off makes no call, shadow counts
// and grants as asked, enforce refuses when nothing is left or org-core cannot
// be read) with one difference: an "ask" host is kept, because the person has
// just answered the question. "Deny", a host org-core did not answer for and a
// "read" host (an extension adds no access level of its own, so a read-only
// verdict cannot be honoured on an act-capable parent) are dropped.
func (s *Server) applyExtensionSitePolicy(ctx context.Context, principal authctx.Principal, domains []string) ([]string, []string, error) {
	mode := s.options.SitePolicy
	if mode == sitepolicy.ModeOff || mode == "" {
		return domains, nil, nil
	}
	shadow := mode == sitepolicy.ModeShadow
	resolution, err := s.resolveSites(ctx, principal, domains)
	if err != nil {
		recordSiteVerdict(ctx, "policy_unavailable")
		if shadow {
			return domains, nil, nil
		}
		telemetry.GrantsDeniedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("reason", "site_policy_unavailable")))
		return nil, nil, status.Error(codes.Unavailable, sitepolicy.ErrUnavailable.Error())
	}
	allowed, denied := splitByExtensionPolicy(domains, resolution)
	switch {
	case len(denied) == 0:
		recordSiteVerdict(ctx, "allowed")
		return domains, nil, nil
	case shadow:
		recordSiteVerdict(ctx, "shadow_would_deny")
		return domains, nil, nil
	case len(allowed) == 0:
		recordSiteVerdict(ctx, "denied")
		telemetry.GrantsDeniedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("reason", "site_policy")))
		return nil, nil, siteDeniedError(denied)
	default:
		recordSiteVerdict(ctx, "narrowed")
		return allowed, reportedDenied(denied), nil
	}
}

// splitByExtensionPolicy keeps the hosts org-core answered "act" or "ask" for.
func splitByExtensionPolicy(domains []string, resolution sitepolicy.Resolution) (allowed, denied []string) {
	effective := make(map[string]string, len(resolution.Decisions))
	for _, decision := range resolution.Decisions {
		effective[decision.Host] = decision.Effective
	}
	for _, host := range domains {
		switch effective[host] {
		case sitepolicy.EffectiveAct, sitepolicy.EffectiveAsk:
			allowed = append(allowed, host)
		default:
			denied = append(denied, host)
		}
	}
	return allowed, denied
}
