package server

import (
	"context"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"

	"google.golang.org/grpc"
)

// Server is the default CapabilityCore implementation backed by an in-memory
// registry and policy engine.
type Server struct {
	mpv1.UnimplementedCapabilityCoreServer
	registry *registry.Registry
	modelReg *registry.ModelsRegistry
	policy   *policy.Engine
}

// toDetail converts a domain Capability into a wire CapabilityDetail.
func toDetail(c *models.Capability) *mpv1.CapabilityDetail {
	if c == nil {
		return nil
	}
	return &mpv1.CapabilityDetail{
		CapabilityId: c.ID,
		Name:         c.Name,
		Kind:         c.Kind,
		Version:      c.Version,
		Description:  c.Description,
		RiskLevel:    c.RiskLevel,
		LazyLoad:     c.LazyLoad,
		Scope:        c.Scope,
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

// ListCapabilities returns a filtered, paginated list of capabilities.
func (s *Server) ListCapabilities(ctx context.Context, req *mpv1.ListCapabilitiesRequest) (*mpv1.ListCapabilitiesResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ListCapabilities")))
	items, hasMore := s.registry.List(req.KindFilter, req.Query, req.AfterId, req.Limit)
	details := make([]*mpv1.CapabilityDetail, 0, len(items))
	for _, c := range items {
		details = append(details, toDetail(c))
	}
	if s.modelReg != nil {
		modelCaps, err := s.modelReg.ListAsCapabilities(ctx, registry.ModelsFilter{OnlyEnabled: true})
		if err != nil {
			return nil, mapErr(err)
		}
		for _, c := range modelCaps {
			details = append(details, toDetail(c))
		}
	}
	return &mpv1.ListCapabilitiesResponse{Capabilities: details, HasMore: hasMore}, nil
}

// GetCapability returns a single capability by ID, honouring an optional version constraint.
func (s *Server) GetCapability(ctx context.Context, req *mpv1.GetCapabilityRequest) (*mpv1.CapabilityDetail, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetCapability")))
	if s.modelReg != nil && strings.HasPrefix(req.CapabilityId, "cap.model.") {
		m, err := s.modelReg.GetByCapabilityID(ctx, req.CapabilityId)
		if err != nil {
			return nil, mapErr(err)
		}
		return toDetail(registry.ToCapability(m)), nil
	}
	c, err := s.registry.Get(req.CapabilityId, req.VersionConstraint)
	if err != nil {
		return nil, mapErr(err)
	}
	return toDetail(c), nil
}

// EvaluatePolicy produces an allow/deny/constrained decision for a capability invocation.
func (s *Server) EvaluatePolicy(ctx context.Context, req *mpv1.EvaluatePolicyRequest) (*mpv1.EvaluatePolicyResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "EvaluatePolicy")))
	result, err := s.policy.Evaluate(ctx, req.CapabilityId, req.RunId, req.AgentId, req.OrgId, req.Scope)
	if err != nil {
		return nil, mapErr(err)
	}
	return &mpv1.EvaluatePolicyResponse{
		Decision:      result.Decision,
		Reason:        result.Reason,
		BudgetContext: result.BudgetContext,
	}, nil
}

// ValidateSkillBundle validates that a skill exists and is structurally promotable.
func (s *Server) ValidateSkillBundle(ctx context.Context, req *mpv1.ValidateSkillBundleRequest) (*mpv1.ValidateSkillBundleResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ValidateSkillBundle")))
	capability, validationErrors, err := s.registry.ValidateSkill(req.SkillId)
	if err != nil {
		return nil, mapErr(err)
	}
	detail := toDetail(capability)
	return &mpv1.ValidateSkillBundleResponse{Valid: len(validationErrors) == 0, Errors: validationErrors, Capability: detail}, nil
}

// CheckSkillPromotion validates whether a skill may move between scopes.
func (s *Server) CheckSkillPromotion(ctx context.Context, req *mpv1.CheckSkillPromotionRequest) (*mpv1.CheckSkillPromotionResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "CheckSkillPromotion")))
	_, checks, err := s.registry.CheckPromotion(req.SkillId, req.FromScope, req.ToScope)
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
	capability, checks, err := s.registry.PromoteSkill(req.SkillId, req.FromScope, req.ToScope)
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

// Register wires the CapabilityCore service onto the provided gRPC server.
func Register(g grpc.ServiceRegistrar, impl mpv1.CapabilityCoreServer) {
	mpv1.RegisterCapabilityCoreServer(g, impl)
}
