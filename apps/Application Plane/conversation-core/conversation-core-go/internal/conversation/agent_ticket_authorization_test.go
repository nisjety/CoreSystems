package conversation

import (
	"strings"
	"testing"
)

func validAgentTicketActionAuthorization() AgentTicketActionAuthorization {
	return AgentTicketActionAuthorization{
		DecisionRef: "decision_1", SpaceRef: "space_1", SubjectID: "user_1",
		RecipientAudienceRef: "audience_1", RecipientAudienceHash: "sha256:audience",
		RecipientAudienceRevision: 1, PrivacyPolicyRef: "privacy_1", AuthorityRevision: 1,
	}
}

func TestAgentTicketActionAuthorizationIsCompleteAndBounded(t *testing.T) {
	if err := validAgentTicketActionAuthorization().Validate(); err != nil {
		t.Fatalf("valid authorization rejected: %v", err)
	}
	for name, mutate := range map[string]func(*AgentTicketActionAuthorization){
		"missing subject":         func(a *AgentTicketActionAuthorization) { a.SubjectID = "" },
		"oversized Space":         func(a *AgentTicketActionAuthorization) { a.SpaceRef = strings.Repeat("s", 201) },
		"zero audience revision":  func(a *AgentTicketActionAuthorization) { a.RecipientAudienceRevision = 0 },
		"zero authority revision": func(a *AgentTicketActionAuthorization) { a.AuthorityRevision = 0 },
	} {
		t.Run(name, func(t *testing.T) {
			authorization := validAgentTicketActionAuthorization()
			mutate(&authorization)
			if err := authorization.Validate(); err == nil {
				t.Fatal("invalid authorization was accepted")
			}
		})
	}
}
