package server

import (
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"testing"
)

func TestPolicyStoreCheckIsIncludedInSignedEvidenceOnlyWhenPerformed(t *testing.T) {
	signer := NewDecisionProofSignerForTest()
	for _, checked := range []bool{true, false} {
		_, claims := signedClaims(t, signer, decisionProofClaimsFor(
			decisionProofRequest{capabilityID: "cap.browser.open", runID: "run-a", agentID: "execution-core", orgID: "org-a", scope: "global", userID: "user-a", toolName: "browser_agent"},
			&decisionProofResponse{decision: "ask", sideEffectClass: "external_write", approvalFloor: "none", policySource: "risk_level", toolPolicyChecked: checked},
		))
		value, present := claims["tool_policy_checked"]
		if checked && (!present || value != true) {
			t.Fatalf("checked proof did not carry the marker: %v", claims)
		}
		if !checked && present {
			t.Fatalf("unchecked proof carried the marker: %v", claims)
		}
	}
}

// The signed marker must come from the actual server evaluation and store read,
// not merely from the proof serialization helper.
func TestScopedBrowserPolicyIsSignedFromTheCheckedStore(t *testing.T) {
	for _, tc := range []struct{ decision, want, source string }{
		{policy.ToolPolicyNever, policy.DecisionDeny, policy.PolicySourceToolPolicyNever},
		{policy.ToolPolicyAsk, policy.DecisionAsk, policy.PolicySourceToolPolicyAsk},
	} {
		t.Run(tc.decision, func(t *testing.T) {
			browser := availableCapabilityRow("cap.browser.open", models.KindTool, models.RiskHigh, models.SideEffectExternalWrite, models.ApprovalFloorNone)
			db := &bindingDB{rows: []bindingRow{{capabilityID: browser.ID, toolName: "browser_agent", class: "external_write", floor: "none", version: "1.0.0"}}}
			server, signer := toolBindingServer(t, db, browser)
			store := &toolPolicyStore{policies: []policy.ToolPolicy{{ScopeKind: policy.ToolPolicyScopeUser, Decision: tc.decision}}}
			server.policy.WithToolPolicies(store, false).WithBrowserAgentPolicyEnforced(true)
			response := evaluateTool(t, server, browser.ID, "browser_agent", "user-a")
			if response.Decision != tc.want || response.PolicySource != tc.source || store.calls != 1 || store.gotUser != "user-a" {
				t.Fatalf("response=%+v store=%+v", response, store)
			}
			claims := evidenceClaims(t, signer, response.DecisionEvidence)
			if claims["tool_policy_checked"] != true || claims["tool_name"] != "browser_agent" || claims["user_id"] != "user-a" || claims["policy_source"] != tc.source {
				t.Fatalf("incorrect signed evidence: %v", claims)
			}
		})
	}
}
