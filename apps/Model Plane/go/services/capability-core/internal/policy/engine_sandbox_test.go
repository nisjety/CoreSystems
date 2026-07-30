package policy_test

import (
	"context"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// executionDispatchSource mirrors the durable execution-dispatch rows as the
// Postgres-backed CapabilitiesSource actually loads them in production: org_id
// 'global', scope 'global', enabled_for_scopes {global}. The static in-process
// seed declares these capabilities with workspace scope, which policy rejects
// before it ever reaches the risk branch, so asserting on the seed shape would
// prove nothing about the live decision.
type executionDispatchSource struct {
	caps []*models.Capability
}

func (s executionDispatchSource) Load() ([]*models.Capability, error) {
	out := make([]*models.Capability, 0, len(s.caps))
	for _, c := range s.caps {
		copied := *c
		out = append(out, &copied)
	}
	return out, nil
}

func globalDispatchCapability(id, name, risk string) *models.Capability {
	return &models.Capability{
		ID:                id,
		Name:              name,
		Kind:              models.KindTool,
		Version:           "1.0.0",
		RiskLevel:         risk,
		Scope:             "global",
		Enabled:           true,
		OrgID:             "global",
		EnabledForScopes:  []string{"global"},
		RolloutState:      "stable",
		AvailabilityState: string(models.AvailabilityUnavailable),
		ReasonCode:        "health_not_attested",
		ExecutionMode:     models.ExecutionUnavailable,
	}
}

// TestEngineAllowsHermeticSandboxWhileArbitraryShellStillAsks is the regression
// guard for the whole point of cap.command.sandbox: the hermetic code executor
// must clear policy without a human in the loop, and arbitrary shell must not.
// If both ever land on the same decision, either every calculator step becomes
// awaiting_approval or arbitrary command execution has quietly lost its gate.
func TestEngineAllowsHermeticSandboxWhileArbitraryShellStillAsks(t *testing.T) {
	t.Parallel()

	reg, err := registry.NewFromSource(executionDispatchSource{caps: []*models.Capability{
		globalDispatchCapability("cap.command.sandbox", "code_interpreter", models.RiskLow),
		globalDispatchCapability("cap.command.shell", "shell", models.RiskHigh),
	}})
	if err != nil {
		t.Fatalf("build registry: %v", err)
	}
	engine := policy.New(reg)
	ctx := context.Background()

	sandbox, err := engine.Evaluate(ctx, "cap.command.sandbox", "run-1", "agent-1", "org-1", "global")
	if err != nil {
		t.Fatalf("evaluate cap.command.sandbox: %v", err)
	}
	if sandbox.Decision != policy.DecisionAllow {
		t.Fatalf("cap.command.sandbox decision = %q (reason=%q), want %q",
			sandbox.Decision, sandbox.Reason, policy.DecisionAllow)
	}
	if sandbox.Decision == policy.DecisionAsk {
		t.Fatal("hermetic sandbox execution must not require human approval")
	}
	if sandbox.Reason != "low-risk capability allowed under default budget" {
		t.Fatalf("unexpected reason: %q", sandbox.Reason)
	}
	if sandbox.BudgetContext != "tokens=100000,cost_usd=5.00" {
		t.Fatalf("cap.command.sandbox budget = %q, want the low-risk default budget",
			sandbox.BudgetContext)
	}

	shell, err := engine.Evaluate(ctx, "cap.command.shell", "run-1", "agent-1", "org-1", "global")
	if err != nil {
		t.Fatalf("evaluate cap.command.shell: %v", err)
	}
	if shell.Decision != policy.DecisionAsk {
		t.Fatalf("cap.command.shell decision = %q (reason=%q), want %q — arbitrary "+
			"command execution must stay human-approved",
			shell.Decision, shell.Reason, policy.DecisionAsk)
	}
	if !strings.Contains(shell.Reason, "high-risk and requires human approval") {
		t.Fatalf("cap.command.shell reason missing high-risk phrase: %q", shell.Reason)
	}
	if shell.BudgetContext != "" {
		t.Fatalf("cap.command.shell budget = %q, want empty on ask", shell.BudgetContext)
	}
}

// TestSeededSandboxCapabilityEvaluatesToAllow proves the in-process static seed
// entry (not just a hand-built snapshot) resolves to allow once evaluated at a
// scope the durable row actually declares.
func TestSeededSandboxCapabilityEvaluatesToAllow(t *testing.T) {
	t.Parallel()

	reg := registry.NewRegistry()
	engine := policy.New(reg)

	seeded, err := reg.Get("cap.command.sandbox", "")
	if err != nil {
		t.Fatalf("expected cap.command.sandbox in the static seed: %v", err)
	}
	if seeded.RiskLevel != models.RiskLow {
		t.Fatalf("seeded cap.command.sandbox risk = %q, want low", seeded.RiskLevel)
	}

	authorized := *seeded
	authorized.Scope = "global"
	authorized.EnabledForScopes = []string{"global"}
	result, err := engine.EvaluateCapability(
		context.Background(), &authorized, "run-1", "agent-1", "triodelab", "global",
	)
	if err != nil {
		t.Fatalf("EvaluateCapability: %v", err)
	}
	if result.Decision != policy.DecisionAllow {
		t.Fatalf("decision = %q (reason=%q), want %q",
			result.Decision, result.Reason, policy.DecisionAllow)
	}
	if result.BudgetContext != "tokens=100000,cost_usd=5.00" {
		t.Fatalf("budget = %q, want the low-risk default budget", result.BudgetContext)
	}
}
