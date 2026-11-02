package server

import (
	"context"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// toolPolicyStore is a policy.ToolPolicySource that answers every call with the same
// stored policies and records whose policies were asked for, so a test can tell
// whether the engine read the store at all and for which person.
type toolPolicyStore struct {
	policies []policy.ToolPolicy
	calls    int
	gotUser  string
}

func (s *toolPolicyStore) ActiveToolPolicies(_ context.Context, _, userID, _ string) ([]policy.ToolPolicy, error) {
	s.calls++
	s.gotUser = userID
	return s.policies, nil
}

// With TOOL_POLICY_ENFORCED off a stored policy is stored and ignored: the decision is
// the binding's, and the store is never read.
func TestEvaluatePolicyIgnoresStoredToolPoliciesWhileTheSwitchIsOff(t *testing.T) {
	fetch := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	db := &bindingDB{rows: []bindingRow{{capabilityID: fetch.ID, toolName: "web_fetch", class: "read", floor: "none", version: "1.0.0"}}}
	server, _ := toolBindingServer(t, db, fetch)
	store := &toolPolicyStore{policies: []policy.ToolPolicy{{ScopeKind: policy.ToolPolicyScopeUser, Decision: policy.ToolPolicyNever}}}
	server.policy.WithToolPolicies(store, false)

	response := evaluateTool(t, server, fetch.ID, "web_fetch", "user-a")
	if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("a stored never changed the decision while the switch was off: %+v", response)
	}
	if store.calls != 0 {
		t.Fatalf("the store was read while the switch was off (%d calls)", store.calls)
	}
}

// With the switch on, a stored policy of the org tightens the call and names itself as
// the decision's source, in the response and in the signed evidence.
func TestEvaluatePolicyAppliesEnforcedStoredPoliciesToTheCall(t *testing.T) {
	for _, tc := range []struct {
		name         string
		decision     string
		wantDecision string
		wantSource   string
	}{
		{name: "a stored never denies", decision: policy.ToolPolicyNever, wantDecision: policy.DecisionDeny, wantSource: policy.PolicySourceToolPolicyNever},
		{name: "a stored ask pauses an allow", decision: policy.ToolPolicyAsk, wantDecision: policy.DecisionAsk, wantSource: policy.PolicySourceToolPolicyAsk},
	} {
		t.Run(tc.name, func(t *testing.T) {
			fetch := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
			db := &bindingDB{rows: []bindingRow{{capabilityID: fetch.ID, toolName: "web_fetch", class: "read", floor: "none", version: "1.0.0"}}}
			server, signer := toolBindingServer(t, db, fetch)
			store := &toolPolicyStore{policies: []policy.ToolPolicy{{ScopeKind: policy.ToolPolicyScopeOrg, Decision: tc.decision}}}
			server.policy.WithToolPolicies(store, true)

			response := evaluateTool(t, server, fetch.ID, "web_fetch", "user-a")
			if response.Decision != tc.wantDecision || response.Reason != tc.wantSource || response.PolicySource != tc.wantSource {
				t.Fatalf("decision = %q, reason = %q, source = %q", response.Decision, response.Reason, response.PolicySource)
			}
			if store.calls != 1 || store.gotUser != "user-a" {
				t.Fatalf("store calls = %d for %q, want one call for user-a", store.calls, store.gotUser)
			}
			claims := evidenceClaims(t, signer, response.DecisionEvidence)
			if claims["decision"] != tc.wantDecision || claims["policy_source"] != tc.wantSource {
				t.Fatalf("evidence disagrees with the response: %v", claims)
			}
		})
	}
}

// A person evaluates only for themselves: naming someone else is refused before any
// stored policy is read. A service (execution-core) asserts the person its run acts for.
func TestEvaluatePolicyPinsTheAssertedUserForPeople(t *testing.T) {
	fetch := availableCapabilityRow("cap.tool.http", models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone)
	db := &bindingDB{rows: []bindingRow{{capabilityID: fetch.ID, toolName: "web_fetch", class: "read", floor: "none", version: "1.0.0"}}}
	server, _ := toolBindingServer(t, db, fetch)
	store := &toolPolicyStore{}
	server.policy.WithToolPolicies(store, true)
	person := authctx.ContextWithPrincipal(context.Background(), authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"})
	request := func(user string) *mpv1.EvaluatePolicyRequest {
		return &mpv1.EvaluatePolicyRequest{CapabilityId: fetch.ID, RunId: "run-a", AgentId: "execution-core", Scope: "global", ToolName: "web_fetch", UserId: user}
	}

	if _, err := server.EvaluatePolicy(person, request("user-b")); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("a person evaluated for someone else: status = %v", status.Code(err))
	}
	if store.calls != 0 {
		t.Fatalf("the stored policies of another person were read (%d calls)", store.calls)
	}
	for _, user := range []string{"", "user-a"} {
		if _, err := server.EvaluatePolicy(person, request(user)); err != nil {
			t.Fatalf("a person evaluated for themselves (%q): %v", user, err)
		}
	}
	if _, err := server.EvaluatePolicy(verifiedGRPCContext(t, "org-a"), request("user-b")); err != nil {
		t.Fatalf("a service asserting a user: %v", err)
	}
	if store.gotUser != "user-b" {
		t.Fatalf("the service's evaluation read the policies of %q, want user-b", store.gotUser)
	}
}
