package server

import (
	"context"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

func newTestServer() *Server {
	reg := registry.NewRegistry()
	pol := policy.New(reg)
	return NewServer(reg, nil, pol)
}

func TestRegisterWiresServiceDescriptor(t *testing.T) {
	g := grpc.NewServer()
	// Register must not panic and the service must be advertised.
	Register(g, newTestServer())
	info := g.GetServiceInfo()
	if _, ok := info["model_plane.v1.CapabilityCore"]; !ok {
		t.Fatalf("expected service model_plane.v1.CapabilityCore to be registered, got %v", info)
	}
}

func TestListCapabilities(t *testing.T) {
	s := newTestServer()
	ctx := context.Background()

	t.Run("no filter returns seeded set", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(resp.Capabilities) == 0 {
			t.Fatalf("expected seeded capabilities, got none")
		}
	})

	t.Run("kind filter narrows results", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{KindFilter: "tool"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		for _, c := range resp.Capabilities {
			if c.Kind != "tool" {
				t.Errorf("expected kind=tool, got %q for %s", c.Kind, c.CapabilityId)
			}
		}
		if len(resp.Capabilities) == 0 {
			t.Fatalf("expected at least one tool capability")
		}
	})

	t.Run("query matches name substring", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{Query: "memory"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(resp.Capabilities) == 0 {
			t.Fatalf("expected memory capabilities, got none")
		}
	})

	t.Run("pagination via limit+afterID", func(t *testing.T) {
		first, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{Limit: 1})
		if err != nil {
			t.Fatalf("first page: %v", err)
		}
		if len(first.Capabilities) != 1 {
			t.Fatalf("expected 1 item, got %d", len(first.Capabilities))
		}
		if !first.HasMore {
			t.Fatalf("expected HasMore=true on first page")
		}
		second, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{Limit: 1, AfterId: first.Capabilities[0].CapabilityId})
		if err != nil {
			t.Fatalf("second page: %v", err)
		}
		if len(second.Capabilities) != 1 {
			t.Fatalf("expected 1 item on page 2, got %d", len(second.Capabilities))
		}
		if second.Capabilities[0].CapabilityId == first.Capabilities[0].CapabilityId {
			t.Errorf("page 2 returned same item as page 1: %s", first.Capabilities[0].CapabilityId)
		}
	})

	t.Run("unknown kind yields empty result", func(t *testing.T) {
		resp, err := s.ListCapabilities(ctx, &mpv1.ListCapabilitiesRequest{KindFilter: "does-not-exist"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if len(resp.Capabilities) != 0 {
			t.Errorf("expected empty, got %d", len(resp.Capabilities))
		}
		if resp.HasMore {
			t.Errorf("expected HasMore=false on empty result")
		}
	})
}

func TestGetCapability(t *testing.T) {
	s := newTestServer()
	ctx := context.Background()

	t.Run("found", func(t *testing.T) {
		detail, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.memory.search"})
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if detail.CapabilityId != "cap.memory.search" {
			t.Errorf("got %s", detail.CapabilityId)
		}
	})

	t.Run("empty id -> InvalidArgument", func(t *testing.T) {
		_, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{})
		if status.Code(err) != codes.InvalidArgument {
			t.Errorf("expected InvalidArgument, got %v", err)
		}
	})

	t.Run("unknown id -> NotFound", func(t *testing.T) {
		_, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.does.not.exist"})
		if status.Code(err) != codes.NotFound {
			t.Errorf("expected NotFound, got %v", err)
		}
	})

	t.Run("version mismatch -> FailedPrecondition", func(t *testing.T) {
		_, err := s.GetCapability(ctx, &mpv1.GetCapabilityRequest{CapabilityId: "cap.memory.search", VersionConstraint: "9.9.9"})
		if status.Code(err) != codes.FailedPrecondition {
			t.Errorf("expected FailedPrecondition, got %v", err)
		}
	})
}

func TestEvaluatePolicy(t *testing.T) {
	s := newTestServer()
	ctx := context.Background()

	baseReq := func(capID string) *mpv1.EvaluatePolicyRequest {
		return &mpv1.EvaluatePolicyRequest{
			CapabilityId: capID,
			RunId:        "run-1",
			AgentId:      "agent-1",
			OrgId:        "org-1",
		}
	}

	t.Run("low-risk -> allow", func(t *testing.T) {
		resp, err := s.EvaluatePolicy(ctx, baseReq("cap.memory.search"))
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if resp.Decision != policy.DecisionAllow {
			t.Errorf("expected allow, got %s", resp.Decision)
		}
		if resp.BudgetContext == "" {
			t.Errorf("expected non-empty budget context")
		}
	})

	t.Run("medium-risk -> allow with constrained budget", func(t *testing.T) {
		resp, err := s.EvaluatePolicy(ctx, baseReq("cap.tool.http"))
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if resp.Decision != policy.DecisionAllow {
			t.Errorf("expected allow, got %s", resp.Decision)
		}
		if resp.BudgetContext == "" {
			t.Errorf("expected constrained budget context")
		}
	})

	t.Run("high-risk -> deny", func(t *testing.T) {
		resp, err := s.EvaluatePolicy(ctx, baseReq("cap.sandbox.exec"))
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		if resp.Decision != policy.DecisionDeny {
			t.Errorf("expected deny, got %s", resp.Decision)
		}
		if resp.Reason == "" {
			t.Errorf("expected non-empty reason for deny")
		}
	})

	t.Run("missing org -> InvalidArgument", func(t *testing.T) {
		req := baseReq("cap.memory.search")
		req.OrgId = ""
		_, err := s.EvaluatePolicy(ctx, req)
		if status.Code(err) != codes.InvalidArgument {
			t.Errorf("expected InvalidArgument, got %v", err)
		}
	})

	t.Run("unknown capability -> NotFound", func(t *testing.T) {
		_, err := s.EvaluatePolicy(ctx, baseReq("cap.does.not.exist"))
		if status.Code(err) != codes.NotFound {
			t.Errorf("expected NotFound, got %v", err)
		}
	})
}

func TestSkillPromotionRPCs(t *testing.T) {
	s := newTestServer()
	ctx := context.Background()

	validation, err := s.ValidateSkillBundle(ctx, &mpv1.ValidateSkillBundleRequest{SkillId: "cap.skill.summarize"})
	if err != nil {
		t.Fatalf("ValidateSkillBundle: %v", err)
	}
	if !validation.Valid {
		t.Fatalf("expected skill to validate, got errors %v", validation.Errors)
	}

	gate, err := s.CheckSkillPromotion(ctx, &mpv1.CheckSkillPromotionRequest{
		SkillId:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})
	if err != nil {
		t.Fatalf("CheckSkillPromotion: %v", err)
	}
	if !gate.Passed {
		t.Fatalf("expected promotion gate to pass, got %+v", gate)
	}

	promoted, err := s.PromoteSkill(ctx, &mpv1.PromoteSkillRequest{
		SkillId:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})
	if err != nil {
		t.Fatalf("PromoteSkill: %v", err)
	}
	if !promoted.Promoted {
		t.Fatalf("expected promotion success, got %+v", promoted)
	}
	if promoted.Capability == nil || promoted.Capability.Scope != "workspace" {
		t.Fatalf("expected promoted scope workspace, got %+v", promoted.Capability)
	}

	failedGate, err := s.CheckSkillPromotion(ctx, &mpv1.CheckSkillPromotionRequest{
		SkillId:   "cap.skill.summarize",
		FromScope: "agent",
		ToScope:   "workspace",
	})
	if err != nil {
		t.Fatalf("CheckSkillPromotion mismatch: %v", err)
	}
	if failedGate.Passed {
		t.Fatalf("expected gate mismatch to fail after promotion, got %+v", failedGate)
	}
}
