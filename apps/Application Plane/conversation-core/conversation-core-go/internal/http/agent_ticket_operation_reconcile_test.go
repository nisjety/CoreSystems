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

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
)

func (r *agentTicketOperationRepository) GetTicketOperation(_ context.Context, orgID, actorUserID, idempotencyKey string) (*conversation.TicketOperationReceipt, error) {
	if orgID != "org_control" || actorUserID != "user_control" || idempotencyKey != "ticket-agent-reconcile" {
		return nil, conversation.ErrNotFound
	}
	if r.intentStatus == "unknown" {
		return &conversation.TicketOperationReceipt{
			OperationID: "ticketop_unknown", Status: "unknown", ControlReservationID: r.reservationID,
			TerminalReason: r.unknownReason, Replayed: true,
		}, nil
	}
	return &conversation.TicketOperationReceipt{
		OperationID: "ticketop_reconciled", AuditEventID: "audit_reconciled", Status: "completed",
		Ticket: &conversation.Ticket{ID: "ticket_reconciled", OrgID: orgID}, Replayed: true,
	}, nil
}

func TestReconcileAgentTicketOperationSurfacesDurableUnknownWithoutTicket(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{intentStatus: "unknown", reservationID: "reservation-unknown", unknownReason: "owner transaction outcome is ambiguous"}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(verifier)
	body := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-reconcile", WorkType: "customer_case"}
	decision := agentTicketTestDecision(body)
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	token := signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	reconcile := agentTicketOperationReconcileBody{
		RunID: body.RunID, OrgID: decision.OrgID, ControlDecisionToken: token,
		ActionSchemaHash: decision.ActionSchemaHash, PayloadDigest: decision.PayloadDigest,
		IdempotencyKey: body.IdempotencyKey,
	}
	wireBody, err := json.Marshal(reconcile)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations/reconcile", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusConflict || !bytes.Contains(response.Body.Bytes(), []byte(`"code":"owner_action_unknown"`)) || bytes.Contains(response.Body.Bytes(), []byte(`"ticket"`)) {
		t.Fatalf("status/body = %d/%s, want conflict with no ticket", response.Code, response.Body.String())
	}
}

func TestReconcileAgentTicketOperationIsReadOnlyAndDecisionBound(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewRunActionDecisionVerifier("control-test-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	repository := &agentTicketOperationRepository{}
	handler := NewHandler(nil, conversation.NewService(repository, nil))
	handler.SetRunActionDecisionVerifier(verifier)
	body := agentTicketCreateBody{RunID: "run_1", ConversationID: "conversation_1", IdempotencyKey: "ticket-agent-reconcile", WorkType: "customer_case"}
	decision := agentTicketTestDecision(body)
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	token := signRunActionDecisionForTest(t, privateKey, "control-test-key", decision)
	reconcile := agentTicketOperationReconcileBody{
		RunID: body.RunID, OrgID: decision.OrgID, ControlDecisionToken: token,
		ActionSchemaHash: decision.ActionSchemaHash, PayloadDigest: decision.PayloadDigest,
		IdempotencyKey: body.IdempotencyKey,
	}
	wireBody, err := json.Marshal(reconcile)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations/reconcile", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	response := performRequest(agentTicketTestRouter(t, handler), request)
	if response.Code != http.StatusOK || !bytes.Contains(response.Body.Bytes(), []byte(`"operation_id":"ticketop_reconciled"`)) {
		t.Fatalf("status/body = %d/%s", response.Code, response.Body.String())
	}
}
