package http

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

type agentTicketGrantRepository struct {
	conversation.Repository
	createInput conversation.CreateAgentTicketActionGrantInput
	revokeInput conversation.RevokeAgentTicketActionGrantInput
}

func (r *agentTicketGrantRepository) CreateAgentTicketActionGrant(_ context.Context, input conversation.CreateAgentTicketActionGrantInput) (*conversation.AgentTicketActionGrantReceipt, error) {
	r.createInput = input
	return &conversation.AgentTicketActionGrantReceipt{
		Grant:        &conversation.AgentTicketActionGrant{ID: "grant_1", OrgID: input.OrgID, ConversationID: input.ConversationID},
		AuditEventID: "audit_grant_1", Status: "created",
	}, nil
}

func (r *agentTicketGrantRepository) RevokeAgentTicketActionGrant(_ context.Context, input conversation.RevokeAgentTicketActionGrantInput) (*conversation.AgentTicketActionGrantReceipt, error) {
	r.revokeInput = input
	return &conversation.AgentTicketActionGrantReceipt{
		Grant:        &conversation.AgentTicketActionGrant{ID: input.GrantID, OrgID: input.OrgID, ConversationID: input.ConversationID},
		AuditEventID: "audit_grant_revoke_1", Status: "revoked",
	}, nil
}

func TestOwnerGrantCreateRequiresExactControlAndGatewayPrincipal(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketGrantRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetOwnerGrantDecisionVerifier(verifier)
	decision := agentTicketGrantDecision("create", "")
	body := ownerGrantBody{IdempotencyKey: decision.IdempotencyKey}
	body.ControlDecisionToken = signOwnerGrantDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/conversation_1/agent-action-grants", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "verevon-gateway", testGatewaySecret, decision.SubjectID, decision.OrgID, "owner")

	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusCreated {
		t.Fatalf("status/body = %d/%s", response.Code, response.Body.String())
	}
	if repository.createInput.OrgID != decision.OrgID || repository.createInput.ConversationID != decision.ConversationID ||
		repository.createInput.CreatedByUserID != decision.SubjectID || repository.createInput.SpaceRef != decision.SpaceRef ||
		repository.createInput.RecipientAudienceHash != decision.RecipientAudienceHash || repository.createInput.AuthorityRevision != decision.AuthorityRevision {
		t.Fatalf("owner grant input did not retain exact signed authority: %#v", repository.createInput)
	}
}

func TestOwnerGrantRejectsReboundPrincipalPathAndOperation(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	for name, candidate := range map[string]struct {
		method string
		path   string
		userID string
		mutate func(*ownerGrantDecision)
	}{
		"different gateway principal": {http.MethodPost, "/api/v1/conversations/conversation_1/agent-action-grants", "other-user", nil},
		"different conversation path": {http.MethodPost, "/api/v1/conversations/conversation_other/agent-action-grants", "user_control", nil},
		"create token cannot revoke":  {http.MethodDelete, "/api/v1/conversations/conversation_1/agent-action-grants/grant_1", "user_control", nil},
		"revoke token wrong grant": {
			http.MethodDelete, "/api/v1/conversations/conversation_1/agent-action-grants/grant_other", "user_control",
			func(d *ownerGrantDecision) {
				d.Operation = "revoke"
				d.GrantID = "grant_1"
				d.Permissions = []string{ownerGrantRevokePermission}
			},
		},
	} {
		t.Run(name, func(t *testing.T) {
			repository := &agentTicketGrantRepository{}
			handler := NewHandler(nil, conversation.NewService(repository, nil))
			handler.SetOwnerGrantDecisionVerifier(verifier)
			decision := agentTicketGrantDecision("create", "")
			if candidate.mutate != nil {
				candidate.mutate(&decision)
			}
			body := ownerGrantBody{IdempotencyKey: decision.IdempotencyKey, ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-test-key", decision)}
			wireBody, err := json.Marshal(body)
			if err != nil {
				t.Fatal(err)
			}
			request := httptest.NewRequest(candidate.method, candidate.path, bytes.NewReader(wireBody))
			signConversationRequest(t, request, wireBody, "verevon-gateway", testGatewaySecret, candidate.userID, decision.OrgID, "owner")
			response := performRequest(agentTicketTestRouter(t, handler), request)
			if response.Code != http.StatusForbidden {
				t.Fatalf("status/body = %d/%s, want forbidden", response.Code, response.Body.String())
			}
			if repository.createInput.OrgID != "" || repository.revokeInput.OrgID != "" {
				t.Fatalf("rebound request reached owner repository: create=%#v revoke=%#v", repository.createInput, repository.revokeInput)
			}
		})
	}
}

func TestOwnerGrantRevokeUsesExactPathBoundDecision(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketGrantRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetOwnerGrantDecisionVerifier(verifier)
	decision := agentTicketGrantDecision("revoke", "grant_1")
	body := ownerGrantBody{IdempotencyKey: decision.IdempotencyKey, ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-test-key", decision)}
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	request := httptest.NewRequest(http.MethodDelete, "/api/v1/conversations/conversation_1/agent-action-grants/grant_1", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "verevon-gateway", testGatewaySecret, decision.SubjectID, decision.OrgID, "admin")
	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusOK {
		t.Fatalf("status/body = %d/%s", response.Code, response.Body.String())
	}
	if repository.revokeInput.GrantID != "grant_1" || repository.revokeInput.SpaceRef != decision.SpaceRef || repository.revokeInput.SubjectID != decision.SubjectID {
		t.Fatalf("revoke input did not retain exact decision/path binding: %#v", repository.revokeInput)
	}
}

func agentTicketGrantDecision(operation, grantID string) ownerGrantDecision {
	permission := ownerGrantCreatePermission
	if operation == "revoke" {
		permission = ownerGrantRevokePermission
	}
	return ownerGrantDecision{
		DecisionRef: "decision_grant_1", OrgID: "org_control", ConversationID: "conversation_1", SpaceRef: "space_personal",
		SubjectID: "user_control", ServiceAudience: ownerGrantDecisionServiceAudience, ActionID: "tickets.create", Operation: operation,
		GrantID: grantID, IdempotencyKey: "owner-grant-1", RecipientAudienceRef: "audience_1", RecipientAudienceHash: "sha256:audience",
		RecipientAudienceRevision: 1, PrivacyPolicyRef: "privacy_1", AuthorityRevision: 1, Permissions: []string{permission},
		Purpose: "support", LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard", Residency: "eu", DeletionScope: "space",
		IssuedAt: time.Now().UTC().Add(-time.Second), ExpiresAt: time.Now().UTC().Add(time.Minute), Nonce: "nonce_1",
	}
}

func signOwnerGrantDecisionForTest(t *testing.T, privateKey ed25519.PrivateKey, keyID string, decision ownerGrantDecision) string {
	t.Helper()
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatalf("Marshal(decision): %v", err)
	}
	signed := ownerGrantDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(privateKey, []byte(signed)))
}
