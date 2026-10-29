package spaces

import (
	"strings"
	"testing"
	"time"
)

func validOwnerGrantRequest() OwnerGrantDecisionRequest {
	return OwnerGrantDecisionRequest{
		ConversationID: "conversation-1", ActionID: "tickets.create", Operation: "create",
		IdempotencyKey: "owner-grant-1", DecisionRef: "decision-1", Nonce: "nonce-1",
	}
}

func TestIssueOwnerGrantDecisionIsPersonalAndOperationBound(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.Membership.Role = "owner"
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)

	decision, err := IssueOwnerGrantDecision(evidence, validOwnerGrantRequest(), now)
	if err != nil {
		t.Fatalf("IssueOwnerGrantDecision: %v", err)
	}
	if decision.Operation != "create" || decision.GrantID != "" || len(decision.Permissions) != 1 || decision.Permissions[0] != ownerGrantCreatePermission {
		t.Fatalf("unexpected create authority: %#v", decision)
	}
	if decision.ServiceAudience != ownerGrantDecisionServiceAudience || decision.ConversationID != "conversation-1" || decision.SubjectID != evidence.Membership.SubjectID {
		t.Fatalf("decision did not preserve exact authority: %#v", decision)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued owner grant decision did not validate: %v", err)
	}
}

func TestIssueOwnerGrantDecisionFailsClosedOnIneligibleOrReboundInputs(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence, *OwnerGrantDecisionRequest){
		"agent action entitlement disabled": func(e *PersonalThreadDecisionEvidence, _ *OwnerGrantDecisionRequest) { e.AgentActionEntitled = false },
		"editor cannot self-approve":        func(e *PersonalThreadDecisionEvidence, _ *OwnerGrantDecisionRequest) { e.Membership.Role = "editor" },
		"shared Space is not yet enabled":   func(e *PersonalThreadDecisionEvidence, _ *OwnerGrantDecisionRequest) { e.Membership.Kind = KindRoom },
		"zero data retention": func(e *PersonalThreadDecisionEvidence, _ *OwnerGrantDecisionRequest) {
			e.Privacy.ZeroDataRetention = true
		},
		"unknown action":              func(_ *PersonalThreadDecisionEvidence, r *OwnerGrantDecisionRequest) { r.ActionID = "users.delete" },
		"create names a grant":        func(_ *PersonalThreadDecisionEvidence, r *OwnerGrantDecisionRequest) { r.GrantID = "grant-1" },
		"revoke lacks an exact grant": func(_ *PersonalThreadDecisionEvidence, r *OwnerGrantDecisionRequest) { r.Operation = "revoke" },
		"oversized conversation": func(_ *PersonalThreadDecisionEvidence, r *OwnerGrantDecisionRequest) {
			r.ConversationID = strings.Repeat("c", 201)
		},
	} {
		t.Run(name, func(t *testing.T) {
			evidence := validPersonalThreadEvidence()
			evidence.AgentActionEntitled = true
			evidence.Membership.Role = "owner"
			request := validOwnerGrantRequest()
			mutate(&evidence, &request)
			if _, err := IssueOwnerGrantDecision(evidence, request, now); err == nil {
				t.Fatal("ineligible grant management request unexpectedly issued a decision")
			}
		})
	}
}
