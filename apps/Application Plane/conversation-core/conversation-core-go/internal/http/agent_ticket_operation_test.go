package http

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
	"github.com/gin-gonic/gin"
)

const agentTicketExecutionSecret = "execution-core-test-secret-at-least-32-bytes"

type agentTicketOperationRepository struct {
	conversation.Repository
	gotInput        conversation.CreateTicketInput
	onCreate        func(conversation.CreateTicketInput)
	intent          *conversation.TicketOperationIntentInput
	reservationID   string
	intentStatus    string
	unknownReason   string
	cancelledReason string
	createErr       error
}

func (r *agentTicketOperationRepository) ResolveAgentTicketActionGrant(_ context.Context, input conversation.CreateTicketInput) (string, error) {
	if input.AgentActionAuthorization == nil {
		return "", conversation.ErrForbidden
	}
	return "grant_agent_1", nil
}

func (r *agentTicketOperationRepository) BeginAgentTicketOperationIntent(_ context.Context, input conversation.TicketOperationIntentInput) (*conversation.TicketOperationReceipt, error) {
	if r.intent != nil {
		if r.intent.OperationID != input.OperationID || r.intent.RequestSHA256 != input.RequestSHA256 || r.intent.GrantRef != input.GrantRef {
			return nil, conversation.ErrConflict
		}
		return &conversation.TicketOperationReceipt{OperationID: input.OperationID, Status: r.intentStatus, ControlReservationID: r.reservationID, Replayed: true}, nil
	}
	r.intentCopy(input)
	r.intentStatus = "pending_control_commit"
	return &conversation.TicketOperationReceipt{OperationID: input.OperationID, Status: r.intentStatus}, nil
}

func (r *agentTicketOperationRepository) BindAgentTicketOperationReservation(_ context.Context, input conversation.TicketOperationReservationInput) (*conversation.TicketOperationReceipt, error) {
	if r.intent == nil || r.intent.OperationID != input.OperationID || r.intent.RequestSHA256 != input.RequestSHA256 || r.intent.GrantRef != input.GrantRef {
		return nil, conversation.ErrConflict
	}
	if r.intentStatus == "completed" {
		return &conversation.TicketOperationReceipt{OperationID: input.OperationID, Status: "completed", Ticket: &conversation.Ticket{ID: "ticket_agent_1"}}, nil
	}
	r.intentStatus = "reserved"
	r.reservationID = input.ControlReservationID
	return &conversation.TicketOperationReceipt{OperationID: input.OperationID, Status: "reserved", ControlReservationID: r.reservationID}, nil
}

func (r *agentTicketOperationRepository) MarkAgentTicketOperationUnknown(_ context.Context, input conversation.TicketOperationOutcomeInput) error {
	r.intentStatus = "unknown"
	r.unknownReason = input.TerminalReason
	r.reservationID = input.ControlReservationID
	return nil
}

func (r *agentTicketOperationRepository) MarkAgentTicketOperationCancelled(_ context.Context, input conversation.TicketOperationOutcomeInput) error {
	r.intentStatus = "cancelled"
	r.cancelledReason = input.TerminalReason
	r.reservationID = input.ControlReservationID
	return nil
}

func (r *agentTicketOperationRepository) intentCopy(input conversation.TicketOperationIntentInput) {
	copy := input
	r.intent = &copy
}

type runActionAuthorityValidatorFunc func(context.Context, runActionDecision) error

func (f runActionAuthorityValidatorFunc) ValidateRunActionAuthority(ctx context.Context, decision runActionDecision) error {
	return f(ctx, decision)
}

type readyOwnerEffectReservationCoordinator struct {
	reserve func(context.Context, string, ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error)
	commit  func(context.Context, string, string, ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error)
}

func (readyOwnerEffectReservationCoordinator) Ready() bool { return true }

func (c readyOwnerEffectReservationCoordinator) Reserve(ctx context.Context, token string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
	if c.reserve != nil {
		return c.reserve(ctx, token, commitment)
	}
	return ownerEffectReservationReceipt{ReservationID: "owner_effect_reservation_1", OperationID: commitment.OperationID, Status: "reserved"}, nil
}

func (c readyOwnerEffectReservationCoordinator) Commit(ctx context.Context, reservationID, token string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
	if c.commit != nil {
		return c.commit(ctx, reservationID, token, commitment)
	}
	return ownerEffectReservationReceipt{ReservationID: reservationID, OperationID: commitment.OperationID, Status: "committed"}, nil
}

func (r *agentTicketOperationRepository) GetConversation(_ context.Context, orgID, conversationID string) (*conversation.ConversationDetail, error) {
	return &conversation.ConversationDetail{ConversationSummary: conversation.ConversationSummary{ID: conversationID, OrgID: orgID}}, nil
}

func (r *agentTicketOperationRepository) CreateTicketOperation(_ context.Context, input conversation.CreateTicketInput) (*conversation.TicketOperationReceipt, error) {
	r.gotInput = input
	if r.createErr != nil {
		return nil, r.createErr
	}
	if r.intent != nil {
		r.intentStatus = "completed"
	}
	if r.onCreate != nil {
		r.onCreate(input)
	}
	return &conversation.TicketOperationReceipt{
		OperationID:  input.OperationID,
		AuditEventID: "audit_agent_ticket_1",
		Status:       "completed",
		Ticket:       &conversation.Ticket{ID: "ticket_agent_1", OrgID: input.OrgID, ConversationID: input.ConversationID},
	}, nil
}

func TestAgentTicketOperationMarksOwnerAmbiguityUnknownBeforeReturning(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{createErr: errors.New("owner connection lost after submit")}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	body := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-owner-unknown", WorkType: "customer_case"}
	decision := agentTicketTestDecision(body)
	decision.IdempotencyKey = body.IdempotencyKey
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusConflict || !bytes.Contains(response.Body.Bytes(), []byte(`"code":"owner_action_unknown"`)) || repository.intentStatus != "unknown" {
		t.Fatalf("status/body/intent = %d/%s/%q, want durable unknown", response.Code, response.Body.String(), repository.intentStatus)
	}
}

func TestAgentTicketOperationCommitsControlReservationBeforeOwnerEffect(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	committed := false
	authorityChecks := 0
	repository := &agentTicketOperationRepository{onCreate: func(input conversation.CreateTicketInput) {
		if !committed || input.AgentActionAuthorization == nil || input.AgentActionAuthorization.ControlReservationID != "owner_effect_reservation_1" || input.AgentActionAuthorization.GrantRef != "grant_agent_1" {
			t.Fatalf("owner effect entered without committed exact reservation: %#v", input.AgentActionAuthorization)
		}
	}}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.SetRunActionAuthorityValidator(runActionAuthorityValidatorFunc(func(context.Context, runActionDecision) error {
		authorityChecks++
		return nil
	}))
	handler.ownerEffectReservationCoordinator = readyOwnerEffectReservationCoordinator{
		reserve: func(_ context.Context, token string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
			if token == "" || commitment.GrantRef != "grant_agent_1" || commitment.OperationID == "" {
				t.Fatalf("reservation commitment = %#v", commitment)
			}
			return ownerEffectReservationReceipt{ReservationID: "owner_effect_reservation_1", OperationID: commitment.OperationID, Status: "reserved"}, nil
		},
		commit: func(_ context.Context, reservationID, token string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
			if reservationID != "owner_effect_reservation_1" || token == "" || commitment.GrantRef != "grant_agent_1" {
				t.Fatalf("commit inputs = %q/%#v", reservationID, commitment)
			}
			committed = true
			return ownerEffectReservationReceipt{ReservationID: reservationID, OperationID: commitment.OperationID, Status: "committed"}, nil
		},
	}
	body := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-reservation", WorkType: "customer_case"}
	decision := agentTicketTestDecision(body)
	decision.IdempotencyKey = body.IdempotencyKey
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusCreated || !committed || authorityChecks != 2 || repository.gotInput.OrgID == "" {
		t.Fatalf("status/commit/authority checks/input = %d/%t/%d/%#v", response.Code, committed, authorityChecks, repository.gotInput)
	}
}

func TestAgentTicketOperationNeverWritesWhenControlCommitIsDenied(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.ownerEffectReservationCoordinator = readyOwnerEffectReservationCoordinator{
		commit: func(context.Context, string, string, ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
			return ownerEffectReservationReceipt{}, ErrOwnerEffectReservationDenied
		},
	}
	body := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-commit-denied", WorkType: "customer_case"}
	decision := agentTicketTestDecision(body)
	decision.IdempotencyKey = body.IdempotencyKey
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusForbidden || repository.gotInput.OrgID != "" {
		t.Fatalf("status/input = %d/%#v, want denied without owner effect", response.Code, repository.gotInput)
	}
}

func TestAgentTicketOperationFinalControlRevocationCancelsBeforeOwnerEffect(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	checks := 0
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.SetRunActionAuthorityValidator(runActionAuthorityValidatorFunc(func(context.Context, runActionDecision) error {
		checks++
		if checks == 2 {
			return ErrRunActionAuthorityDenied
		}
		return nil
	}))
	handler.ownerEffectReservationCoordinator = readyOwnerEffectReservationCoordinator{}
	body := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-final-revoke", WorkType: "customer_case"}
	decision := agentTicketTestDecision(body)
	decision.IdempotencyKey = body.IdempotencyKey
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusForbidden || checks != 2 || repository.gotInput.OrgID != "" || repository.intentStatus != "cancelled" {
		t.Fatalf("status/checks/input/intent = %d/%d/%#v/%q, want final revocation cancellation", response.Code, checks, repository.gotInput, repository.intentStatus)
	}
}

func agentTicketTestRouter(t *testing.T, handler *Handler) *gin.Engine {
	t.Helper()
	// The test router exercises the pre-reservation handler bindings with a
	// controlled coordinator fixture. Production construction intentionally has
	// no coordinator until the durable pending/reserve/commit/finalize protocol
	// is wired, and therefore remains fail-closed.
	if handler.ownerEffectReservationCoordinator == nil {
		handler.ownerEffectReservationCoordinator = readyOwnerEffectReservationCoordinator{}
	}
	if handler.runActionAuthorityValidator == nil {
		handler.runActionAuthorityValidator = runActionAuthorityValidatorFunc(func(context.Context, runActionDecision) error { return nil })
	}
	verifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "conversation-core",
		Keys: map[string]string{
			"verevon-gateway":     testGatewaySecret,
			"conversation-ingest": testIngestSecret,
			"execution-core":      agentTicketExecutionSecret,
		},
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	return newRouter(handler, verifier)
}

func TestAgentTicketOperationFailsClosedWithoutControlDecisionVerifier(t *testing.T) {
	handler := NewHandler(nil, nil)
	body := []byte(`{"run_id":"run_1","control_decision_token":"untrusted","conversation_id":"conversation_1","idempotency_key":"ticket-agent-1"}`)
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(body))
	signConversationRequest(t, request, body, "execution-core", agentTicketExecutionSecret, "forged-user", "forged-org", "owner")

	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("status/body = %d/%s, want unavailable", response.Code, response.Body.String())
	}
}

func TestAgentTicketOperationFailsClosedWithoutCurrentControlAuthorityValidator(t *testing.T) {
	publicKey, _, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, nil)
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.ownerEffectReservationCoordinator = readyOwnerEffectReservationCoordinator{}
	verifier, err := delegation.NewVerifier(delegation.Config{
		Audience: "conversation-core",
		Keys:     map[string]string{"execution-core": agentTicketExecutionSecret},
	})
	if err != nil {
		t.Fatalf("NewVerifier() error = %v", err)
	}
	body := []byte(`{"run_id":"run_1","control_decision_token":"untrusted","conversation_id":"conversation_1","idempotency_key":"ticket-agent-1"}`)
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(body))
	signConversationRequest(t, request, body, "execution-core", agentTicketExecutionSecret, "", "", "")
	response := performRequest(newRouter(handler, verifier), request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("status/body = %d/%s, want unavailable", response.Code, response.Body.String())
	}
}

func TestAgentTicketOperationRequiresExactControlDecisionAndUsesItsAuthority(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.SetRunActionAuthorityValidator(runActionAuthorityValidatorFunc(func(context.Context, runActionDecision) error { return nil }))

	body := agentTicketCreateBody{
		RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-1",
		WorkType: "customer_case", Priority: "high", Category: "refund", Intent: "review_refund",
	}
	decision := agentTicketTestDecision(body)
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	// These verified delegation claims are intentionally forged relative to the
	// signed Control decision. The owner action must use the decision instead.
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "forged-user", "forged-org", "owner")

	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusCreated {
		t.Fatalf("status/body = %d/%s", response.Code, response.Body.String())
	}
	if repository.gotInput.OrgID != decision.OrgID || repository.gotInput.ActorUserID != decision.SubjectID || repository.gotInput.CreatedBy != decision.SubjectID {
		t.Fatalf("owner input authority = %#v, want Control decision authority", repository.gotInput)
	}
	if repository.gotInput.Source != "approved_ai_action" || repository.gotInput.ConversationID != body.ConversationID || repository.gotInput.IdempotencyKey != body.IdempotencyKey {
		t.Fatalf("owner input = %#v", repository.gotInput)
	}
	if repository.gotInput.AgentActionAuthorization == nil ||
		repository.gotInput.AgentActionAuthorization.DecisionRef != decision.DecisionRef ||
		repository.gotInput.AgentActionAuthorization.SpaceRef != decision.SpaceRef ||
		repository.gotInput.AgentActionAuthorization.SubjectID != decision.SubjectID ||
		repository.gotInput.AgentActionAuthorization.RecipientAudienceRef != decision.RecipientAudienceRef ||
		repository.gotInput.AgentActionAuthorization.RecipientAudienceHash != decision.RecipientAudienceHash ||
		repository.gotInput.AgentActionAuthorization.RecipientAudienceRevision != decision.RecipientAudienceRevision ||
		repository.gotInput.AgentActionAuthorization.PrivacyPolicyRef != decision.PrivacyPolicyRef ||
		repository.gotInput.AgentActionAuthorization.AuthorityRevision != decision.AuthorityRevision {
		t.Fatalf("agent target authorization = %#v, want exact signed decision facts", repository.gotInput.AgentActionAuthorization)
	}
}

func TestAgentTicketOperationRejectsControlReservationDenialBeforeOwnerEffect(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.ownerEffectReservationCoordinator = readyOwnerEffectReservationCoordinator{
		reserve: func(context.Context, string, ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
			return ownerEffectReservationReceipt{}, ErrOwnerEffectReservationDenied
		},
	}

	body := agentTicketCreateBody{
		RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-authority-change",
		WorkType: "customer_case", Priority: "normal",
	}
	decision := agentTicketTestDecision(body)
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")

	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("status/body = %d/%s, want reservation denial", response.Code, response.Body.String())
	}
	if repository.gotInput.OrgID != "" {
		t.Fatalf("owner effect received stale-authority input: %#v", repository.gotInput)
	}
}

func TestAgentTicketOperationRejectsReboundPayloadBeforeOwnerEffect(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)

	approved := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-1", WorkType: "customer_case", Priority: "normal"}
	decision := agentTicketTestDecision(approved)
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, approved)
	approved.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	// Change a decision-bound field after signing. This must be rejected before
	// Conversation Core gets a chance to create an owner receipt.
	approved.ConversationID = "conversation_other"
	wireBody, err := json.Marshal(approved)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")

	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("status/body = %d/%s, want denied", response.Code, response.Body.String())
	}
	if repository.gotInput.OrgID != "" {
		t.Fatalf("owner effect received rebound input: %#v", repository.gotInput)
	}
}

func TestAgentTicketContinuationRejectsOwnerRebindBeforeOwnerEffect(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)

	body := agentTicketCreateBody{
		RunID: "run_1", OwnerUserID: "different-user", ConversationID: "conversation_1",
		IdempotencyKey: "ticket-agent-continuation-owner", WorkType: "customer_case",
	}
	decision := agentTicketTestDecision(body)
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")

	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusForbidden {
		t.Fatalf("status/body = %d/%s, want owner rebind denial", response.Code, response.Body.String())
	}
	if repository.gotInput.OrgID != "" {
		t.Fatalf("owner effect received rebound continuation: %#v", repository.gotInput)
	}
}

func TestAgentTicketOperationRejectsAmbiguousOrExtendedJSONBeforeVerification(t *testing.T) {
	publicKey, _, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, conversation.NewService(&agentTicketOperationRepository{}, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	for _, body := range [][]byte{
		[]byte(`{"run_id":"run_1","control_decision_token":"invalid","conversation_id":"conversation_1","idempotency_key":"ticket-agent-1","unexpected":"field"}`),
		[]byte(`{"run_id":"run_1","control_decision_token":"invalid","conversation_id":"conversation_1","idempotency_key":"ticket-agent-1"} {}`),
	} {
		request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(body))
		signConversationRequest(t, request, body, "execution-core", agentTicketExecutionSecret, "", "", "")
		response := performRequest(agentTicketTestRouter(t, handler), request)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("status/body = %d/%s, want strict JSON rejection", response.Code, response.Body.String())
		}
	}
}

func TestAgentTicketOperationCarriesOwnerAuthorizationToTheDurableBoundary(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.SetRunActionAuthorityValidator(runActionAuthorityValidatorFunc(func(context.Context, runActionDecision) error { return nil }))
	body := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-1", WorkType: "customer_case"}
	decision := agentTicketTestDecision(body)
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")

	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusCreated || repository.gotInput.AgentActionAuthorization == nil {
		t.Fatalf("status/authorization = %d/%#v, want durable target authorization", response.Code, repository.gotInput.AgentActionAuthorization)
	}
}

func agentTicketTestDecision(body agentTicketCreateBody) runActionDecision {
	return runActionDecision{
		DecisionRef: "decision_1", RunID: body.RunID, ThreadID: "thread_1", OrgID: "org_control", SpaceRef: "space_1", SubjectID: "user_control",
		ServiceAudience: runActionDecisionServiceAudience, ActionID: "tickets.create", ActionSchemaHash: ticketCreateActionContract().SchemaSHA256,
		IdempotencyKey: body.IdempotencyKey, RecipientAudienceRef: "audience_1", RecipientAudienceHash: "audience-hash", RecipientAudienceRevision: 1,
		PrivacyPolicyRef: "privacy_1", RunContextAuthorizationRef: "control:space_1:thread-create:1", AuthorityRevision: 1,
		Permissions: []string{runActionDecisionPermission}, Purpose: "support", LawfulBasis: "contract", PrivacyClass: "internal",
		ThirdPartyAllowed: false, RetentionClass: "standard", Residency: "eu", DeletionScope: "space", ZeroDataRetention: false,
		IssuedAt: time.Now().UTC().Add(-time.Second), ExpiresAt: time.Now().UTC().Add(time.Minute), Nonce: "nonce_1",
	}
}

func signRunActionDecisionForTest(t *testing.T, privateKey ed25519.PrivateKey, keyID string, decision runActionDecision) string {
	t.Helper()
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatalf("Marshal(decision) error = %v", err)
	}
	signed := runActionDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(privateKey, []byte(signed)))
}
