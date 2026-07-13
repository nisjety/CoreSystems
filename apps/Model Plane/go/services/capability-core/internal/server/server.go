package server

import (
	"context"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// Server is the default CapabilityCore implementation backed by the registry,
// policy engine, and (optionally) the durable capabilities store used for
// score-ranked listing.
type Server struct {
	mpv1.UnimplementedCapabilityCoreServer
	registry *registry.Registry
	modelReg *registry.ModelsRegistry
	policy   *policy.Engine
	store    capabilityStore // optional: enables score-ranked List
}

type capabilityStore interface {
	GetForOrg(context.Context, string, string) (*registry.CapabilityRow, error)
	RankedList(context.Context, string, string, []string, int) ([]registry.ScoredCapability, error)
}

// toDetail converts a domain Capability into a wire CapabilityDetail.
func toDetail(c *models.Capability) *mpv1.CapabilityDetail {
	if c == nil {
		return nil
	}
	availability := models.DeriveAvailability(c)
	return &mpv1.CapabilityDetail{
		CapabilityId:     c.ID,
		Name:             c.Name,
		Kind:             c.Kind,
		Version:          c.Version,
		Description:      c.Description,
		RiskLevel:        c.RiskLevel,
		LazyLoad:         c.LazyLoad,
		Scope:            c.Scope,
		State:            string(availability.State),
		ReasonCode:       availability.ReasonCode,
		Reason:           availability.Reason,
		RequiresApproval: availability.RequiresApproval,
		ExecutionMode:    availability.ExecutionMode,
		CostClass:        availability.CostClass,
		HealthCheckedAt:  availability.HealthCheckedAt,
	}
}

func hasAllChecks(checks []string, required ...string) bool {
	set := make(map[string]struct{}, len(checks))
	for _, check := range checks {
		set[check] = struct{}{}
	}
	for _, requiredCheck := range required {
		if _, ok := set[requiredCheck]; !ok {
			return false
		}
	}
	return true
}

// NewServer constructs a Server wired to the provided registry and policy engine.
func NewServer(reg *registry.Registry, modelReg *registry.ModelsRegistry, pol *policy.Engine) *Server {
	return &Server{registry: reg, modelReg: modelReg, policy: pol}
}

// WithStore attaches the durable capabilities store. When present,
// ListCapabilities ranks results by composite score (descending) instead of the
// registry's kind/name order. Returns the same Server for chaining; a nil store
// leaves the in-memory ordering in place.
func (s *Server) WithStore(store *registry.CapabilitiesStore) *Server {
	s.store = store
	return s
}

// ListCapabilities returns a filtered, paginated list of capabilities. When a
// durable store is attached it returns results ranked by composite score
// (descending) so the agentic loop sees the healthiest/safest capabilities
// first; otherwise it falls back to the in-memory registry's kind/name order.
func (s *Server) ListCapabilities(ctx context.Context, req *mpv1.ListCapabilitiesRequest) (*mpv1.ListCapabilitiesResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ListCapabilities")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if s.store != nil {
		return s.listRanked(ctx, req, orgID)
	}
	items, hasMore := s.registry.ListForOrg(orgID, req.KindFilter, req.Query, req.AfterId, req.Limit)
	details := make([]*mpv1.CapabilityDetail, 0, len(items))
	for _, c := range items {
		details = append(details, toDetail(c))
	}
	if s.modelReg != nil {
		modelCaps, err := s.modelReg.ListAsCapabilitiesForOrg(ctx, orgID)
		if err != nil {
			return nil, mapErr(err)
		}
		for _, c := range modelCaps {
			details = append(details, toDetail(c))
		}
	}
	return &mpv1.ListCapabilitiesResponse{Capabilities: details, HasMore: hasMore}, nil
}

// listRanked serves ListCapabilities from the durable store, ordered by
// descending composite score. The text query filters by name/description; the
// kind filter narrows by kind; AfterId is an id cursor over the ranked order.
func (s *Server) listRanked(ctx context.Context, req *mpv1.ListCapabilitiesRequest, orgID string) (*mpv1.ListCapabilitiesResponse, error) {
	limit := int(req.Limit)
	if limit <= 0 || limit > 200 {
		limit = 50
	}
	// Pull a generous tenant-filtered window (including global entries).
	// Over-fetch so the query filter + cursor can still fill a page.
	scored, err := s.store.RankedList(ctx, orgID, req.KindFilter, nil, limit*4+200)
	if err != nil {
		return nil, mapErr(err)
	}

	q := strings.ToLower(strings.TrimSpace(req.Query))
	filtered := make([]*registry.CapabilityRow, 0, len(scored))
	for _, sc := range scored {
		if q != "" &&
			!strings.Contains(strings.ToLower(sc.Row.Name), q) &&
			!strings.Contains(strings.ToLower(sc.Row.Description), q) {
			continue
		}
		filtered = append(filtered, sc.Row)
	}

	start := 0
	if req.AfterId != "" {
		for i, r := range filtered {
			if r.ID == req.AfterId {
				start = i + 1
				break
			}
		}
	}
	end := start + limit
	hasMore := false
	if end < len(filtered) {
		hasMore = true
	} else {
		end = len(filtered)
	}
	if start > len(filtered) {
		start = len(filtered)
	}

	details := make([]*mpv1.CapabilityDetail, 0, end-start)
	for _, r := range filtered[start:end] {
		details = append(details, rowToDetail(r))
	}
	return &mpv1.ListCapabilitiesResponse{Capabilities: details, HasMore: hasMore}, nil
}

// rowToDetail converts a durable CapabilityRow into a wire CapabilityDetail.
func rowToDetail(r *registry.CapabilityRow) *mpv1.CapabilityDetail {
	return toDetail(rowToCapability(r))
}

func rowToCapability(r *registry.CapabilityRow) *models.Capability {
	if r == nil {
		return nil
	}
	return &models.Capability{
		ID:                r.ID,
		Name:              r.Name,
		Kind:              r.Kind,
		Version:           r.Version,
		Description:       r.Description,
		RiskLevel:         r.RiskLevel,
		LazyLoad:          r.LazyLoad,
		Scope:             r.Scope,
		Enabled:           r.Enabled,
		IdempotencyKey:    r.IdempotencyKey,
		OrgID:             r.OrgID,
		EnabledForScopes:  append([]string(nil), r.EnabledForScopes...),
		RolloutState:      r.RolloutState,
		AvailabilityState: r.AvailabilityState,
		ReasonCode:        r.ReasonCode,
		Reason:            r.Reason,
		ExecutionMode:     r.ExecutionMode,
		CostClass:         r.CostClass,
		HealthCheckedAt:   r.HealthCheckedAt,
	}
}

// GetCapability returns a single capability by ID, honouring an optional version constraint.
func (s *Server) GetCapability(ctx context.Context, req *mpv1.GetCapabilityRequest) (*mpv1.CapabilityDetail, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetCapability")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if s.modelReg != nil && strings.HasPrefix(req.CapabilityId, "cap.model.") {
		m, err := s.modelReg.GetByCapabilityIDForOrg(ctx, req.CapabilityId, orgID)
		if err != nil {
			return nil, mapErr(err)
		}
		return toDetail(registry.ToCapability(m)), nil
	}
	if s.store != nil {
		row, err := s.store.GetForOrg(ctx, req.CapabilityId, orgID)
		if err != nil {
			return nil, mapErr(err)
		}
		if req.VersionConstraint != "" && row.Version != req.VersionConstraint {
			return nil, mapErr(domain.ErrVersionMismatch)
		}
		return rowToDetail(row), nil
	}
	c, err := s.registry.GetForOrg(req.CapabilityId, req.VersionConstraint, orgID)
	if err != nil {
		return nil, mapErr(err)
	}
	return toDetail(c), nil
}

// EvaluatePolicy produces an allow/deny/constrained decision for a capability invocation.
func (s *Server) EvaluatePolicy(ctx context.Context, req *mpv1.EvaluatePolicyRequest) (*mpv1.EvaluatePolicyResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "EvaluatePolicy")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if req == nil || !policy.IsSupportedScope(req.Scope) {
		return nil, mapErr(domain.ErrInvalidArgument)
	}
	var (
		capability   *models.Capability
		rolloutState string
	)
	if s.store != nil {
		row, lookupErr := s.store.GetForOrg(ctx, req.CapabilityId, orgID)
		if lookupErr != nil {
			return nil, mapErr(lookupErr)
		}
		if row == nil || (row.OrgID != orgID && row.OrgID != "global") {
			return nil, mapErr(domain.ErrCapabilityNotFound)
		}
		capability = rowToCapability(row)
		rolloutState = row.RolloutState
	} else {
		capability, err = s.registry.GetForOrg(req.CapabilityId, "", orgID)
		if err != nil {
			return nil, mapErr(err)
		}
	}
	if reason := capabilityPolicyBlockReason(capability, rolloutState); reason != "" {
		return &mpv1.EvaluatePolicyResponse{Decision: policy.DecisionDeny, Reason: reason}, nil
	}
	result, err := s.policy.EvaluateCapability(ctx, capability, req.RunId, req.AgentId, orgID, req.Scope)
	if err != nil {
		return nil, mapErr(err)
	}
	return &mpv1.EvaluatePolicyResponse{
		Decision:      result.Decision,
		Reason:        result.Reason,
		BudgetContext: result.BudgetContext,
	}, nil
}

func capabilityPolicyBlockReason(capability *models.Capability, rolloutState string) string {
	switch strings.TrimSpace(rolloutState) {
	case "", "stable", "canary":
	case "quarantine":
		return "rollout_quarantine"
	case "deprecated":
		return "rollout_deprecated"
	default:
		return "invalid_rollout_state"
	}

	availability := models.DeriveAvailability(capability)
	if availability.State == models.AvailabilityAvailable {
		return ""
	}
	if availability.State == models.AvailabilityApprovalRequired {
		return "human_approval_required"
	}
	if availability.ReasonCode != "" {
		return availability.ReasonCode
	}
	return "runtime_unavailable"
}

// ValidateSkillBundle validates that a skill exists and is structurally promotable.
func (s *Server) ValidateSkillBundle(ctx context.Context, req *mpv1.ValidateSkillBundleRequest) (*mpv1.ValidateSkillBundleResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ValidateSkillBundle")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	capability, validationErrors, err := s.registry.ValidateSkillForOrg(req.SkillId, orgID)
	if err != nil {
		return nil, mapErr(err)
	}
	detail := toDetail(capability)
	return &mpv1.ValidateSkillBundleResponse{Valid: len(validationErrors) == 0, Errors: validationErrors, Capability: detail}, nil
}

// CheckSkillPromotion validates whether a skill may move between scopes.
func (s *Server) CheckSkillPromotion(ctx context.Context, req *mpv1.CheckSkillPromotionRequest) (*mpv1.CheckSkillPromotionResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "CheckSkillPromotion")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	_, checks, err := s.registry.CheckPromotionForOrg(req.SkillId, req.FromScope, req.ToScope, orgID)
	if err != nil {
		return nil, mapErr(err)
	}
	passed := hasAllChecks(checks,
		"skill_exists",
		"skill_valid",
		"source_scope_matches",
		"target_scope_valid",
		"scope_changes",
	)
	reason := "promotion checks passed"
	if !passed {
		reason = "promotion checks failed"
	}
	return &mpv1.CheckSkillPromotionResponse{Passed: passed, Checks: checks, Reason: reason}, nil
}

// PromoteSkill updates the registry scope for a validated skill promotion.
func (s *Server) PromoteSkill(ctx context.Context, req *mpv1.PromoteSkillRequest) (*mpv1.PromoteSkillResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "PromoteSkill")))
	orgID, err := verifiedOrganizationID(ctx)
	if err != nil {
		return nil, err
	}
	if s.store != nil {
		return nil, status.Error(codes.FailedPrecondition, "durable skill promotion is not configured")
	}
	capability, checks, err := s.registry.PromoteSkillForOrg(req.SkillId, req.FromScope, req.ToScope, orgID)
	if err != nil {
		return nil, mapErr(err)
	}
	promoted := capability != nil && capability.Scope == req.ToScope && len(checks) > 0 && checks[len(checks)-1] == "registry_updated"
	detail := toDetail(capability)
	reason := "skill promoted"
	if !promoted {
		reason = "promotion requirements not met"
	}
	return &mpv1.PromoteSkillResponse{Promoted: promoted, Checks: checks, Reason: reason, Capability: detail}, nil
}

func verifiedOrganizationID(ctx context.Context) (string, error) {
	principal, ok := authctx.PrincipalFromContext(ctx)
	if !ok || strings.TrimSpace(principal.OrganizationID) == "" {
		return "", status.Error(codes.Unauthenticated, "verified identity required")
	}
	return principal.OrganizationID, nil
}

// Register wires the CapabilityCore service onto the provided gRPC server.
func Register(g grpc.ServiceRegistrar, impl mpv1.CapabilityCoreServer) {
	mpv1.RegisterCapabilityCoreServer(g, impl)
}
