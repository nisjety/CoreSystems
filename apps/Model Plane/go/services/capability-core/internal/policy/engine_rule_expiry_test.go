package policy

import (
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// A confirmed "Always" rule is a user allow that lapses (build plan WS13 S7, K3-10). Read after its
// lapse, or against a changed definition, it is the stale allow it has become: an ask, with the base
// decision and its source unchanged.

// lapsingAllow is a user allow pinned to digestA that lapses at expiresAt. The zero time never lapses.
func lapsingAllow(expiresAt time.Time) ToolPolicy {
	return ToolPolicy{ScopeKind: ToolPolicyScopeUser, Decision: ToolPolicyAllow, PinnedDigest: digestA, ExpiresAt: expiresAt}
}

func TestAnUnexpiredRuleRelaxesTheRiskAsk(t *testing.T) {
	got := DecideWithPolicies(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite),
		[]ToolPolicy{lapsingAllow(time.Now().Add(time.Hour))})
	if got.Decision != DecisionAllow || got.PolicySource != PolicySourceToolPolicyAllow {
		t.Fatalf("an unexpired rule should relax the risk ask: got %+v", got)
	}
}

func TestAnExpiredRuleIsDemotedToAsk(t *testing.T) {
	got := DecideWithPolicies(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite),
		[]ToolPolicy{lapsingAllow(time.Now().Add(-time.Second))})
	if got.Decision != DecisionAsk || got.PolicySource != PolicySourceRiskLevel {
		t.Fatalf("a lapsed rule should read as the base ask: got %+v", got)
	}
}

func TestADigestDriftDemotesTheRuleToAsk(t *testing.T) {
	binding := mcpBinding(models.SideEffectExternalWrite)
	binding.ToolDigest = digestB
	got := DecideWithPolicies(askBase(), mcpCapability(models.RiskHigh), binding,
		[]ToolPolicy{lapsingAllow(time.Now().Add(time.Hour))})
	if got.Decision != DecisionAsk || got.PolicySource != PolicySourceRiskLevel {
		t.Fatalf("a changed definition should read the rule as an ask: got %+v", got)
	}
}

func TestAPolicyWithNoExpiryNeverLapses(t *testing.T) {
	got := DecideWithPolicies(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite),
		[]ToolPolicy{lapsingAllow(time.Time{})})
	if got.Decision != DecisionAllow {
		t.Fatalf("a policy with no expiry should relax the risk ask: got %+v", got)
	}
}

func TestALapsedRuleNeverTightensOrRelaxesOnItsOwn(t *testing.T) {
	// A lapsed allow beside a live ask still asks: the lapse removes a relaxation, nothing else.
	policies := []ToolPolicy{lapsingAllow(time.Now().Add(-time.Hour)), {ScopeKind: ToolPolicyScopeUser, Decision: ToolPolicyAsk}}
	got := DecideWithPolicies(askBase(), mcpCapability(models.RiskHigh), mcpBinding(models.SideEffectExternalWrite), policies)
	if got.Decision != DecisionAsk || got.PolicySource != PolicySourceToolPolicyAsk {
		t.Fatalf("a live ask over a lapsed allow should be reported as the stored ask: got %+v", got)
	}
}
