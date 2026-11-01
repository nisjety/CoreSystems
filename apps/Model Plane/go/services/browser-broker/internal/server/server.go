package server

import (
	"context"
	"os"
	"strings"
	"sync/atomic"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/sitepolicy"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// Server implements the BrowserBroker gRPC service over a grant Repository.
type Server struct {
	mpv1.UnimplementedBrowserBrokerServer
	grants    grant.Repository
	cfg       Config
	baseURL   string
	principal func(context.Context) (authctx.Principal, error)
	options   Options
	// revokedKeys remembers recently revoked API keys (INNH-10).
	revokedKeys *revokedKeys
	// keyGrantsDisabled is set by DisableKeyGrants (INNH-10).
	keyGrantsDisabled atomic.Bool
}

// Options is the optional wiring a broker main sets after NewServerWithConfig.
// The zero value is the default: the site policy is off and a requested
// lifetime is capped at DefaultMaxRequestTTL.
type Options struct {
	// SitePolicy and SiteResolver say how org-core's site decision is read at
	// mint (WS21-S4). Off makes no call.
	SitePolicy   sitepolicy.Mode
	SiteResolver sitepolicy.Resolver
	// MaxRequestTTL caps a caller's requested grant lifetime
	// (BROWSER_GRANT_MAX_TTL).
	MaxRequestTTL time.Duration
}

// WithOptions sets the optional wiring and returns the server.
func (s *Server) WithOptions(options Options) *Server {
	s.options = options
	return s
}

// NewServer constructs a Server with the default Config.
func NewServer(grants grant.Repository) *Server {
	return NewServerWithConfig(grants, DefaultConfig())
}

// NewServerWithConfig constructs a Server with explicit limits.
func NewServerWithConfig(grants grant.Repository, cfg Config) *Server {
	baseURL := strings.TrimRight(os.Getenv("BROWSER_BROKER_BASE_URL"), "/")
	if baseURL == "" {
		baseURL = "https://browser-broker.local"
	}
	if cfg.Policy == nil {
		cfg.Policy = grant.StaticPolicy{
			MaxDomains:         cfg.MaxSessionDomains,
			HumanBrowseEnabled: cfg.HumanBrowseEnabled,
			DeniedSuffixes:     cfg.DenySuffixes,
		}
	}
	return &Server{
		grants: grants, cfg: cfg, baseURL: baseURL, principal: authz.Principal,
		revokedKeys: newRevokedKeys(nil),
	}
}

// AcquireGrant mints a new browser grant using the proto-defined session/mode contract.
func (s *Server) AcquireGrant(ctx context.Context, req *AcquireGrantRequest) (*AcquireGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "AcquireGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetSessionKey() == "" {
		return nil, status.Error(codes.InvalidArgument, "session_key is required")
	}
	if len(req.GetSessionKey()) > maxSessionKeyBytes || strings.ContainsFunc(req.GetSessionKey(), isControl) {
		return nil, status.Error(codes.InvalidArgument, "session_key must be a bounded, printable key")
	}
	if req.GetOrgId() == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id is required")
	}
	if req.GetOrgId() != principal.OrganizationID {
		return nil, status.Error(codes.PermissionDenied, "organization does not match verified identity")
	}
	scopeURL, err := scopeURLForMode(req.GetMode())
	if err != nil {
		return nil, err
	}
	// An API key (INNH-10) has its own path and never reaches the user or
	// service rules below.
	if principal.PrincipalType == authz.PrincipalAPIKey {
		return s.acquireKeyGrant(ctx, req, scopeURL, principal)
	}
	// grant_kind (WS8.C): "" is the ordinary path below; human_browse is a
	// different grant with its own rules; anything else is refused.
	switch req.GetGrantKind() {
	case "":
	case grantKindHumanBrowse:
		// The human_browse path has no read level (WS21-S4 narrows only allowlist
		// grants), so a read-only ask is refused rather than silently widened.
		if req.GetReadOnly() {
			return nil, status.Error(codes.InvalidArgument, "a human_browse grant cannot be read-only")
		}
		return s.acquireHumanBrowse(ctx, req, scopeURL, principal)
	default:
		return nil, status.Error(codes.InvalidArgument, "grant_kind must be empty or human_browse")
	}
	allowedDomains, err := grant.NormalizeAllowedDomains(req.GetAllowedDomains())
	if err != nil {
		return nil, status.Error(codes.InvalidArgument, "allowed_domains must contain bounded hostnames")
	}
	if err := s.cfg.Policy.CheckSession(ctx, principal.OrganizationID, allowedDomains); err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	// ZDR is a floor: the verified token can raise it, the request can raise
	// it, nothing here can lower it.
	zdr := req.GetZdr() || principal.ZeroDataRetention
	allowedActions, allowedFrameIDs, allowedDialogIDs, allowedArtifactIDs :=
		req.GetAllowedActions(), req.GetAllowedFrameIds(), req.GetAllowedDialogIds(), req.GetAllowedArtifactIds()
	if _, err := grant.NormalizeSensitiveScopes(
		allowedActions,
		allowedFrameIDs,
		allowedDialogIDs,
		allowedArtifactIDs,
	); err != nil {
		return nil, status.Error(codes.InvalidArgument, "invalid sensitive browser approval scope")
	}
	budget, err := budgetFromRequest(
		req.GetBudgetMicroUsd(), req.GetBudgetMaxSteps(), req.GetBudgetMaxRuntimeS(),
		req.GetBudgetSource(), req.GetBudgetMintedAtUnix(),
	)
	if err != nil {
		return nil, err
	}
	// A sensitive approval is single-action authority; a spend bound on it
	// would mean nothing and is refused rather than ignored.
	if len(allowedActions) != 0 && budget.IsSet() {
		return nil, status.Error(codes.InvalidArgument, "a sensitive browser approval cannot carry a budget claim")
	}
	readOnly := req.GetReadOnly()
	if readOnly && len(allowedActions) != 0 {
		return nil, status.Error(codes.InvalidArgument, "a sensitive browser approval cannot be read-only")
	}
	// The durable store has no read-only column yet, so a durable grant would
	// report full access. Refuse rather than drop the narrowing silently.
	if readOnly && !zdr {
		return nil, status.Error(codes.FailedPrecondition, "a read-only browser grant must be Zero Data Retention until the durable store persists it")
	}
	var deniedDomains []string
	var g *grant.Grant
	if len(allowedActions) == 0 {
		// ttl_seconds only shortens a run grant. A sensitive approval is single-action
		// authority and always takes the configured TTL, whatever the caller asks for.
		ttl, ttlErr := s.requestTTL(req.GetTtlSeconds())
		if ttlErr != nil {
			return nil, ttlErr
		}
		allowedDomains, deniedDomains, err = s.applySitePolicy(ctx, principal, allowedDomains, readOnly)
		if err != nil {
			return nil, err
		}
		g, err = s.grants.CreateSession(ctx, grant.CreateSessionParams{
			OrgID:             principal.OrganizationID,
			OwnerID:           principal.ActorID,
			SessionKey:        req.GetSessionKey(),
			ScopeURL:          scopeURL,
			AllowedDomains:    allowedDomains,
			TTL:               ttl,
			Zdr:               zdr,
			ReadOnly:          readOnly,
			Budget:            budget,
			MaxActivePerOwner: s.cfg.MaxActivePerOwner,
		})
	} else {
		if req.GetParentGrantId() == "" {
			return nil, status.Error(codes.InvalidArgument, "sensitive browser approval requires parent_grant_id")
		}
		g, err = s.grants.CreateScopedSensitiveApproval(ctx, grant.ApprovalParams{
			OrgID:              principal.OrganizationID,
			OwnerID:            principal.ActorID,
			SessionKey:         req.GetSessionKey(),
			ScopeURL:           scopeURL,
			AllowedDomains:     allowedDomains,
			AllowedActions:     allowedActions,
			AllowedFrameIDs:    allowedFrameIDs,
			AllowedDialogIDs:   allowedDialogIDs,
			AllowedArtifactIDs: allowedArtifactIDs,
			ParentGrantID:      req.GetParentGrantId(),
			TTL:                s.cfg.TTL,
			Zdr:                zdr,
		})
	}
	if err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	logGrantIssued(ctx, g)
	resp := s.acquireResponse(g)
	resp.ReadOnly = g.ReadOnly
	// An allowlist grant has no deny suffixes of its own, so the hosts the site
	// policy dropped (WS21-S4) are the hosts it may never reach.
	resp.DeniedDomains = append(resp.DeniedDomains, deniedDomains...)
	return resp, nil
}

// RevokeGrant marks an existing grant as revoked.
func (s *Server) RevokeGrant(ctx context.Context, req *RevokeGrantRequest) (*RevokeGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "RevokeGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetGrantId() == "" {
		return nil, status.Error(codes.InvalidArgument, "grant_id is required")
	}
	if err := s.keyMayRevoke(ctx, principal, req.GetGrantId()); err != nil {
		telemetry.GrantsRevokedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	if err := s.grants.RevokeScoped(
		ctx, req.GetGrantId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetReason(),
	); err != nil {
		telemetry.GrantsRevokedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsRevokedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "revoked")))
	logGrantRevoked(ctx, req.GetGrantId())
	return &RevokeGrantResponse{Revoked: true}, nil
}

// ValidateGrant checks that a grant exists, is not revoked, and is not expired.
func (s *Server) ValidateGrant(ctx context.Context, req *ValidateGrantRequest) (*ValidateGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ValidateGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetGrantId() == "" {
		return nil, status.Error(codes.InvalidArgument, "grant_id is required")
	}
	g, err := s.grants.GetScoped(ctx, req.GetGrantId(), principal.OrganizationID, authz.OwnerFilter(principal))
	if err == nil {
		err = s.keyGrantLive(principal, g)
	}
	if err != nil {
		telemetry.GrantsValidatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsValidatedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "ok")))
	return &ValidateGrantResponse{
		GrantId:            g.ID,
		Active:             true,
		ExpiresAt:          timestamppb.New(g.ExpiresAt),
		AllowedDomains:     append([]string(nil), g.AllowedDomains...),
		AllowedActions:     append([]string(nil), g.AllowedActions...),
		AllowedFrameIds:    append([]string(nil), g.AllowedFrameIDs...),
		AllowedDialogIds:   append([]string(nil), g.AllowedDialogIDs...),
		AllowedArtifactIds: append([]string(nil), g.AllowedArtifactIDs...),
		ParentGrantId:      g.ParentGrantID,
		Kind:               string(g.Kind),
		OwnerId:            g.OwnerID,
		Zdr:                g.Zdr,
		BudgetMicroUsd:     g.Budget.MicroUSD,
		BudgetMaxSteps:     g.Budget.MaxSteps,
		BudgetMaxRuntimeS:  g.Budget.MaxRuntimeS,
		BudgetSource:       g.Budget.Source,
		BudgetMintedAtUnix: g.Budget.MintedAtUnix,
		PolicyMode:         policyModeOf(g),
		DeniedDomains:      append([]string(nil), g.DeniedDomains...),
		AbsoluteExpiresAt:  timestamppb.New(absoluteOf(g)),
		ResourceId:         g.ResourceID,
		SessionKey:         g.SessionKey,
		ReadOnly:           g.ReadOnly,
		KeyId:              g.KeyID,
	}, nil
}

// derivePurposeAIRun and derivePurposeAttach are the only DeriveRunGrant
// purposes. A new purpose is an explicit addition, never an accidental pass.
const derivePurposeAIRun = "ai_run"

// maxSessionKeyBytes bounds a run's idempotency key.
const maxSessionKeyBytes = 256

// DeriveRunGrant issues a run grant for one agent run from an active session
// grant. The grant layer enforces that the run grant is never wider than its
// parent; this handler checks the request shape and the verified tenant.
func (s *Server) DeriveRunGrant(ctx context.Context, req *DeriveRunGrantRequest) (*DeriveRunGrantResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "DeriveRunGrant")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	// A key grant is never derived from (INNH-10), and a key never derives.
	if principal.PrincipalType == authz.PrincipalAPIKey {
		return nil, status.Error(codes.PermissionDenied, "an api key cannot derive a run grant")
	}
	if req.GetParentGrantId() == "" {
		return nil, status.Error(codes.InvalidArgument, "parent_grant_id is required")
	}
	sessionKey := req.GetSessionKey()
	if strings.TrimSpace(sessionKey) == "" || len(sessionKey) > maxSessionKeyBytes || strings.ContainsFunc(sessionKey, isControl) {
		return nil, status.Error(codes.InvalidArgument, "session_key must be a bounded, printable key")
	}
	if req.GetOrgId() == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id is required")
	}
	if req.GetOrgId() != principal.OrganizationID {
		return nil, status.Error(codes.PermissionDenied, "organization does not match verified identity")
	}
	purpose := req.GetPurpose()
	if purpose != derivePurposeAIRun && purpose != derivePurposeAttach {
		return nil, status.Error(codes.InvalidArgument, "purpose must be ai_run or attach")
	}
	maxHosts := s.cfg.MaxRunDomains
	if purpose == derivePurposeAttach {
		// An attach grant is lent to an agent for one run: only the person who
		// holds the parent may attach, the run is named, and the hosts are few.
		if principal.PrincipalType != "user" {
			return nil, status.Error(codes.PermissionDenied, "an attach grant is derived by a signed-in user only")
		}
		if strings.TrimSpace(req.GetResourceId()) == "" {
			return nil, status.Error(codes.InvalidArgument, "resource_id is required for an attach grant")
		}
		maxHosts = s.cfg.MaxAttachDomains
	} else if req.GetResourceId() != "" {
		return nil, status.Error(codes.InvalidArgument, "resource_id is only for an attach grant")
	}
	ttl, err := s.deriveTTL(req.GetTtlSeconds())
	if err != nil {
		return nil, err
	}
	budget, err := budgetFromRequest(
		req.GetBudgetMicroUsd(), req.GetBudgetMaxSteps(), req.GetBudgetMaxRuntimeS(),
		req.GetBudgetSource(), req.GetBudgetMintedAtUnix(),
	)
	if err != nil {
		return nil, err
	}
	g, err := s.grants.Derive(ctx, grant.DeriveParams{
		OrgID:         principal.OrganizationID,
		OwnerID:       authz.OwnerFilter(principal),
		ParentGrantID: req.GetParentGrantId(),
		SessionKey:    sessionKey,
		RunDomains:    req.GetAllowedDomains(),
		TTL:           ttl,
		PrincipalZdr:  principal.ZeroDataRetention,
		Budget:        budget,
		Purpose:       purpose,
		ResourceID:    req.GetResourceId(),
		MaxRunDomains: maxHosts,
		Policy:        s.cfg.Policy,
	})
	if err != nil {
		telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", grantOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.GrantsIssuedTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "derived")))
	return &DeriveRunGrantResponse{
		GrantId:        g.ID,
		Endpoint:       s.baseURL + "/grants/" + g.ID,
		ExpiresAt:      timestamppb.New(g.ExpiresAt),
		AllowedDomains: append([]string(nil), g.AllowedDomains...),
		ParentGrantId:  g.ParentGrantID,
		Kind:           string(g.Kind),
		Zdr:            g.Zdr,
		ResourceId:     g.ResourceID,
		// The run grant's claim as stored: the request's, tightened by the
		// parent's, or the parent's when the request set none.
		BudgetMicroUsd:     g.Budget.MicroUSD,
		BudgetMaxSteps:     g.Budget.MaxSteps,
		BudgetMaxRuntimeS:  g.Budget.MaxRuntimeS,
		BudgetSource:       g.Budget.Source,
		BudgetMintedAtUnix: g.Budget.MintedAtUnix,
	}, nil
}

// budgetFromRequest reads the budget claim fields of a request. The stamp is
// the broker's: a request that sets it is refused rather than overwritten, so
// a caller cannot believe it chose the claim's age. Shape errors (negative
// bound, unknown source, a source with nothing to bound) are InvalidArgument.
func budgetFromRequest(microUSD int64, maxSteps, maxRuntimeS int32, source string, mintedAtUnix int64) (grant.Budget, error) {
	budget, err := grant.NormalizeBudget(grant.Budget{
		MicroUSD:     microUSD,
		MaxSteps:     maxSteps,
		MaxRuntimeS:  maxRuntimeS,
		Source:       source,
		MintedAtUnix: mintedAtUnix,
	})
	if err != nil {
		return grant.Budget{}, status.Error(codes.InvalidArgument, "invalid budget claim: bounds must not be negative, budget_source must be request, org_default or entitlement exactly when a bound is set, and budget_minted_at_unix is set by the broker")
	}
	return budget, nil
}

// deriveTTL resolves a requested run-grant lifetime: 0 is the configured
// default, a value under the minimum is refused, anything else is clamped to
// the broker maximum. The parent's expiry caps it again in the grant layer.
func (s *Server) deriveTTL(seconds int32) (time.Duration, error) {
	switch {
	case seconds == 0:
		return s.cfg.TTL, nil
	case seconds < 0 || time.Duration(seconds)*time.Second < grant.MinDerivedLifetime:
		return 0, status.Error(codes.InvalidArgument, "ttl_seconds is below the minimum run grant lifetime")
	default:
		return min(time.Duration(seconds)*time.Second, MaxGrantTTL), nil
	}
}

func isControl(r rune) bool { return r <= 0x1f || r == 0x7f }

// Health returns service health status.
func (s *Server) Health(ctx context.Context, _ *BrowserHealthRequest) (*BrowserHealthResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "Health")))
	if _, err := s.principal(ctx); err != nil {
		return nil, err
	}
	return &BrowserHealthResponse{Status: "ok"}, nil
}

// Register wires the BrowserBroker service onto the provided gRPC server.
func Register(g *grpc.Server, impl mpv1.BrowserBrokerServer) {
	mpv1.RegisterBrowserBrokerServer(g, impl)
}

func scopeURLForMode(mode string) (string, error) {
	switch mode {
	case "", "cloud":
		return "browser://cloud", nil
	case "local":
		return "browser://local", nil
	default:
		return "", status.Errorf(codes.InvalidArgument, "unsupported browser mode %q", mode)
	}
}

// DefaultMaxRequestTTL caps a caller's requested grant lifetime when
// BROWSER_GRANT_MAX_TTL is unset.
const DefaultMaxRequestTTL = 30 * time.Minute

// maxDeniedDomainsReported bounds the hosts a refusal or a narrowed grant names:
// the start host and at most eight more, the most a grant's fence may hold.
const maxDeniedDomainsReported = 9

// siteDeniedReason is the ErrorInfo reason of a refusal by the site policy.
const siteDeniedReason = "BROWSER_SITE_DENIED"

// requestTTL resolves a caller's requested lifetime. Zero is the configured
// default; a value under the minimum is refused, and one above the cap is
// clamped to it.
func (s *Server) requestTTL(seconds int32) (time.Duration, error) {
	if seconds == 0 {
		return s.cfg.TTL, nil
	}
	if seconds < 0 || time.Duration(seconds)*time.Second < MinGrantTTL {
		return 0, status.Error(codes.InvalidArgument, "ttl_seconds is below the minimum grant lifetime")
	}
	limit := s.options.MaxRequestTTL
	if limit <= 0 {
		limit = DefaultMaxRequestTTL
	}
	return min(time.Duration(seconds)*time.Second, limit), nil
}

// applySitePolicy narrows an ordinary grant's hosts to what org-core allows for
// this caller at the requested level. It returns the hosts to grant and the
// hosts it dropped. Off makes no call. Shadow counts the verdict and grants as
// asked. Enforce grants only what is allowed, and refuses the mint when nothing
// is left or org-core cannot be read: a broker must not mint from a policy it
// cannot see.
func (s *Server) applySitePolicy(ctx context.Context, principal authctx.Principal, domains []string, readOnly bool) ([]string, []string, error) {
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
	allowed, denied := splitByPolicy(domains, resolution, readOnly)
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

func (s *Server) resolveSites(ctx context.Context, principal authctx.Principal, domains []string) (sitepolicy.Resolution, error) {
	if s.options.SiteResolver == nil {
		return sitepolicy.Resolution{}, sitepolicy.ErrUnavailable
	}
	return s.options.SiteResolver.Resolve(ctx, principal.OrganizationID, authz.OwnerFilter(principal), domains)
}

// splitByPolicy keeps the hosts org-core allows at the requested level. A read
// grant takes read and act hosts (the grant is then read-only); an act grant
// takes act hosts only. Ask and deny are dropped: an ask host has no member
// choice yet, and the member must be asked before anything runs. A host that
// org-core did not answer for is dropped too.
func splitByPolicy(domains []string, resolution sitepolicy.Resolution, readOnly bool) (allowed, denied []string) {
	effective := make(map[string]string, len(resolution.Decisions))
	for _, decision := range resolution.Decisions {
		effective[decision.Host] = decision.Effective
	}
	for _, host := range domains {
		if permitsLevel(effective[host], readOnly) {
			allowed = append(allowed, host)
		} else {
			denied = append(denied, host)
		}
	}
	return allowed, denied
}

func permitsLevel(effective string, readOnly bool) bool {
	switch effective {
	case sitepolicy.EffectiveAct:
		return true
	case sitepolicy.EffectiveRead:
		return readOnly
	default:
		return false
	}
}

func reportedDenied(denied []string) []string {
	if len(denied) > maxDeniedDomainsReported {
		denied = denied[:maxDeniedDomainsReported]
	}
	return append([]string(nil), denied...)
}

// siteDeniedError refuses a mint the site policy allows for none of its hosts.
// The denied hosts ride in the error details, not the message, so a log line
// that carries the message carries no host.
func siteDeniedError(denied []string) error {
	st := status.New(codes.PermissionDenied, "browser site policy denies every requested domain")
	withDetails, err := st.WithDetails(&errdetails.ErrorInfo{
		Reason:   siteDeniedReason,
		Metadata: map[string]string{"denied_domains": strings.Join(reportedDenied(denied), ",")},
	})
	if err != nil {
		return st.Err()
	}
	return withDetails.Err()
}

func recordSiteVerdict(ctx context.Context, outcome string) {
	telemetry.SitePolicyVerdictsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", outcome)))
}
