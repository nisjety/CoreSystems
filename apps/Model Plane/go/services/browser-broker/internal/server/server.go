package server

import (
	"context"
	"os"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"github.com/triodelab/model-plane/services/browser-broker/internal/telemetry"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
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
	return &Server{grants: grants, cfg: cfg, baseURL: baseURL, principal: authz.Principal}
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
	// grant_kind (WS8.C): "" is the ordinary path below; human_browse is a
	// different grant with its own rules; anything else is refused.
	switch req.GetGrantKind() {
	case "":
	case grantKindHumanBrowse:
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
	var g *grant.Grant
	if len(allowedActions) == 0 {
		g, err = s.grants.CreateSession(ctx, grant.CreateSessionParams{
			OrgID:             principal.OrganizationID,
			OwnerID:           principal.ActorID,
			SessionKey:        req.GetSessionKey(),
			ScopeURL:          scopeURL,
			AllowedDomains:    allowedDomains,
			TTL:               s.cfg.TTL,
			Zdr:               zdr,
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
	return s.acquireResponse(g), nil
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
