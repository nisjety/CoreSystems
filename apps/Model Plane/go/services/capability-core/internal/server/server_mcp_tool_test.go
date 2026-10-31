package server

import (
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// A drifted or unapproved MCP tool is refused by the policy gate with the cause
// the tool table recorded, so the caller sees tool_definition_changed rather
// than a generic unavailable (WS05-S2).
func TestPolicyBlockReasonNamesADriftedOrUnapprovedMCPTool(t *testing.T) {
	checkedAt := time.Now().UTC()
	row := func(enabled bool, reasonCode string) *models.Capability {
		return &models.Capability{
			ID: "cap.mcp_tool.x", Kind: models.KindMCPTool, Enabled: enabled, RiskLevel: models.RiskHigh,
			RolloutState: "stable", AvailabilityState: string(models.AvailabilityAvailable),
			ExecutionMode: models.ExecutionAgentic, CostClass: models.CostVariable,
			ReasonCode: reasonCode, HealthCheckedAt: &checkedAt,
		}
	}
	if got := capabilityPolicyBlockReason(row(false, models.ReasonToolDefinitionChanged), "stable"); got != "tool_definition_changed" {
		t.Fatalf("drifted tool reason = %q", got)
	}
	if got := capabilityPolicyBlockReason(row(false, models.ReasonToolNotApproved), "stable"); got != "tool_not_approved" {
		t.Fatalf("pending tool reason = %q", got)
	}
	// An approved, recently observed tool goes on to the policy engine, where its
	// high risk makes the decision an ask.
	if got := capabilityPolicyBlockReason(row(true, ""), "stable"); got != "" {
		t.Fatalf("an approved tool was blocked: %q", got)
	}
	// A tool the gateway stopped seeing is blocked as stale, not allowed on an old sighting.
	stale := row(true, "")
	old := checkedAt.Add(-2 * time.Hour)
	stale.HealthCheckedAt = &old
	if got := capabilityPolicyBlockReason(stale, "stable"); got != "tool_observation_stale" {
		t.Fatalf("stale tool reason = %q", got)
	}
}
