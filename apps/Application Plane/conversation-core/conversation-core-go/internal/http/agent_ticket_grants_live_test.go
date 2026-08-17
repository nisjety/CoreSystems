package http

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/database"
)

type ownerGrantHTTPResponse struct {
	Data struct {
		Grant        conversation.AgentTicketActionGrant `json:"grant"`
		AuditEventID string                              `json:"audit_event_id"`
		Status       string                              `json:"status"`
		Replayed     bool                                `json:"replayed"`
	} `json:"data"`
}

func testLiveConversationCorePool(t *testing.T, orgID, inboxID, conversationID string) *database.DB {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live owner grant proof test")
	}
	ctx := context.Background()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(func() {
		db.Close()
	})

	if err := database.RunMigrations(ctx, db); err != nil {
		t.Fatalf("run migrations: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO conversation_inboxes (id, org_id, name, channel)
VALUES ($1, $2, 'Proof Inbox', 'email')
ON CONFLICT (id) DO UPDATE SET
	org_id = EXCLUDED.org_id, name = EXCLUDED.name, channel = EXCLUDED.channel`, inboxID, orgID); err != nil {
		t.Fatalf("seed inbox: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO conversations (id, org_id, inbox_id, title, status, priority)
VALUES ($1, $2, $3, 'Owner grant proof conversation', 'open', 'normal')
ON CONFLICT (id) DO UPDATE SET org_id=$2, inbox_id=$3, title='Owner grant proof conversation'`, conversationID, orgID, inboxID); err != nil {
		t.Fatalf("seed conversation: %v", err)
	}

	t.Cleanup(func() {
		_, _ = db.Pool.Exec(ctx, `DELETE FROM conversation_ticket_operations WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID)
		_, _ = db.Pool.Exec(ctx, `DELETE FROM conversation_tickets WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID)
		_, _ = db.Pool.Exec(ctx, `DELETE FROM conversation_events WHERE conversation_id = $1`, conversationID)
		_, _ = db.Pool.Exec(ctx, `DELETE FROM conversation_audit_events WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID)
		_, _ = db.Pool.Exec(ctx, `DELETE FROM conversation_agent_action_grants WHERE conversation_id = $1`, conversationID)
		_, _ = db.Pool.Exec(ctx, `DELETE FROM conversations WHERE id = $1`, conversationID)
		_, _ = db.Pool.Exec(ctx, `DELETE FROM conversation_inboxes WHERE id = $1`, inboxID)
	})

	return db
}

func TestLiveOwnerGrantCreateRevokeLifecycle(t *testing.T) {
	orgID := fmt.Sprintf("proof-org-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-conversation-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-inbox-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-user-%d", time.Now().UnixNano()+41)
	spaceRef := fmt.Sprintf("space-%s", orgID)
	recipientAudienceRef := fmt.Sprintf("audience-%s", orgID)
	recipientAudienceHash := fmt.Sprintf("hash-%d", time.Now().UnixNano()+53)

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-proof-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, service)
	handler.SetOwnerGrantDecisionVerifier(verifier)
	router := newRouter(handler, testVerifier(t))

	createDecision := agentTicketGrantDecision("create", "")
	createDecision.OrgID = orgID
	createDecision.ConversationID = conversationID
	createDecision.SpaceRef = spaceRef
	createDecision.SubjectID = subjectID
	createDecision.RecipientAudienceRef = recipientAudienceRef
	createDecision.RecipientAudienceHash = recipientAudienceHash
	createDecision.IdempotencyKey = "owner-grant-create-" + conversationID
	createBody := ownerGrantBody{
		IdempotencyKey:       createDecision.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", createDecision),
	}
	wireCreateBody, err := json.Marshal(createBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}

	createRequest := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/"+conversationID+"/agent-action-grants", bytes.NewReader(wireCreateBody))
	signConversationRequest(t, createRequest, wireCreateBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	createResponse := performRequest(router, createRequest)
	if createResponse.Code != http.StatusCreated {
		t.Fatalf("create status/body = %d/%s", createResponse.Code, createResponse.Body.String())
	}
	var createReceipt ownerGrantHTTPResponse
	if err := json.Unmarshal(createResponse.Body.Bytes(), &createReceipt); err != nil {
		t.Fatalf("create response unmarshal error = %v", err)
	}
	if createReceipt.Data.Status != "created" || createReceipt.Data.Grant.ID == "" || createReceipt.Data.Grant.RevokedAt != nil {
		t.Fatalf("create response grant = %#v", createReceipt.Data)
	}
	var grantRevokedAt *time.Time
	if err := db.Pool.QueryRow(ctx, `
SELECT revoked_at FROM conversation_agent_action_grants WHERE id = $1 AND org_id = $2 AND conversation_id = $3`,
		createReceipt.Data.Grant.ID, orgID, conversationID).Scan(&grantRevokedAt); err != nil {
		t.Fatalf("read grant after create = %v", err)
	}
	if grantRevokedAt != nil {
		t.Fatal("new grant is revoked")
	}

	// Idempotent create with the same idempotency and signed decision must replay
	// the same grant instead of inserting a duplicate row.
	replayRequest := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/"+conversationID+"/agent-action-grants", bytes.NewReader(wireCreateBody))
	signConversationRequest(t, replayRequest, wireCreateBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	replayResponse := performRequest(router, replayRequest)
	if replayResponse.Code != http.StatusCreated {
		t.Fatalf("replay status/body = %d/%s", replayResponse.Code, replayResponse.Body.String())
	}
	var replayReceipt ownerGrantHTTPResponse
	if err := json.Unmarshal(replayResponse.Body.Bytes(), &replayReceipt); err != nil {
		t.Fatalf("replay response unmarshal error = %v", err)
	}
	if replayReceipt.Data.Grant.ID != createReceipt.Data.Grant.ID || !replayReceipt.Data.Replayed {
		t.Fatalf("expected replayed grant to return same grant id, got %#v", replayReceipt.Data)
	}

	// The proof is only complete if the repository-enforced owner-resource
	// requirement allows a ticket operation when the grant is active. The
	// operation schema deliberately requires a complete owner-effect evidence
	// tuple once an agent authorization is present; this synthetic committed
	// reservation represents the Control half while this test focuses on the
	// Conversation Core grant/transaction boundary.
	ticketBody := agentTicketCreateBody{
		RunID: "run-" + conversationID, ConversationID: conversationID,
		IdempotencyKey: "ticket-op-" + conversationID, WorkType: "customer_case",
	}
	ownerEffectAuthorization := &conversation.AgentTicketActionAuthorization{
		DecisionRef:               createDecision.DecisionRef,
		SpaceRef:                  createDecision.SpaceRef,
		SubjectID:                 createDecision.SubjectID,
		RecipientAudienceRef:      createDecision.RecipientAudienceRef,
		RecipientAudienceHash:     createDecision.RecipientAudienceHash,
		RecipientAudienceRevision: createDecision.RecipientAudienceRevision,
		PrivacyPolicyRef:          createDecision.PrivacyPolicyRef,
		AuthorityRevision:         createDecision.AuthorityRevision,
		ControlReservationID:      "reservation-" + conversationID,
		GrantRef:                  createReceipt.Data.Grant.ID,
		ActionSchemaHash:          ticketCreateActionContract().SchemaSHA256,
		PayloadDigest:             agentTicketPayloadDigest(ticketBody.RunID, orgID, ticketBody),
	}
	createTicketInput := conversation.CreateTicketInput{
		OrgID:                    orgID,
		ConversationID:           conversationID,
		WorkType:                 "customer_case",
		ActorUserID:              subjectID,
		IdempotencyKey:           "ticket-op-" + conversationID,
		AgentActionAuthorization: ownerEffectAuthorization,
	}
	createTicketInput.ActionID = "tickets.create"
	createTicketInput.OperationID = conversation.TicketOperationID(orgID, createTicketInput.IdempotencyKey)
	createTicketInput.RequestSHA256 = conversation.TicketOperationRequestSHA256(createTicketInput)
	if _, err := service.BeginAgentTicketOperationIntent(ctx, conversation.TicketOperationIntentInput{
		OperationID: createTicketInput.OperationID, OrgID: orgID, IdempotencyKey: createTicketInput.IdempotencyKey,
		ActionID: "tickets.create", ActorUserID: subjectID, ConversationID: conversationID,
		RequestSHA256: createTicketInput.RequestSHA256, ActionSchemaHash: ownerEffectAuthorization.ActionSchemaHash,
		PayloadDigest: ownerEffectAuthorization.PayloadDigest, DecisionRef: ownerEffectAuthorization.DecisionRef,
		GrantRef: ownerEffectAuthorization.GrantRef,
	}); err != nil {
		t.Fatalf("begin ticket owner intent = %v", err)
	}
	if _, err := service.BindAgentTicketOperationReservation(ctx, conversation.TicketOperationReservationInput{
		TicketOperationIntentInput: conversation.TicketOperationIntentInput{
			OperationID: createTicketInput.OperationID, OrgID: orgID, IdempotencyKey: createTicketInput.IdempotencyKey,
			ActionID: "tickets.create", ActorUserID: subjectID, ConversationID: conversationID,
			RequestSHA256: createTicketInput.RequestSHA256, ActionSchemaHash: ownerEffectAuthorization.ActionSchemaHash,
			PayloadDigest: ownerEffectAuthorization.PayloadDigest, DecisionRef: ownerEffectAuthorization.DecisionRef,
			GrantRef: ownerEffectAuthorization.GrantRef,
		},
		ControlReservationID: ownerEffectAuthorization.ControlReservationID,
	}); err != nil {
		t.Fatalf("bind ticket owner reservation = %v", err)
	}
	if _, err := service.CreateTicketOperation(ctx, createTicketInput); err != nil {
		var postStatus string
		var postTicketID, postAuditID *string
		_ = db.Pool.QueryRow(ctx, `SELECT status, ticket_id, audit_event_id FROM conversation_ticket_operations WHERE operation_id = $1`, createTicketInput.OperationID).Scan(&postStatus, &postTicketID, &postAuditID)
		t.Fatalf("ticket operation with active grant = %v (row status=%s ticket=%v audit=%v)", err, postStatus, postTicketID, postAuditID)
	}

	revokeDecision := agentTicketGrantDecision("revoke", createReceipt.Data.Grant.ID)
	revokeDecision.OrgID = orgID
	revokeDecision.ConversationID = conversationID
	revokeDecision.SpaceRef = spaceRef
	revokeDecision.SubjectID = subjectID
	revokeDecision.IdempotencyKey = "owner-grant-revoke-" + conversationID
	revokeDecision.RecipientAudienceRef = recipientAudienceRef
	revokeDecision.RecipientAudienceHash = recipientAudienceHash
	revokeBody := ownerGrantBody{
		IdempotencyKey:       revokeDecision.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", revokeDecision),
	}
	wireRevokeBody, err := json.Marshal(revokeBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	revokeRequest := httptest.NewRequest(http.MethodDelete, "/api/v1/conversations/"+conversationID+"/agent-action-grants/"+createReceipt.Data.Grant.ID, bytes.NewReader(wireRevokeBody))
	signConversationRequest(t, revokeRequest, wireRevokeBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	revokeResponse := performRequest(router, revokeRequest)
	if revokeResponse.Code != http.StatusOK {
		t.Fatalf("revoke status/body = %d/%s", revokeResponse.Code, revokeResponse.Body.String())
	}
	var revokeReceipt ownerGrantHTTPResponse
	if err := json.Unmarshal(revokeResponse.Body.Bytes(), &revokeReceipt); err != nil {
		t.Fatalf("revoke response unmarshal error = %v", err)
	}
	if revokeReceipt.Data.Status != "revoked" || revokeReceipt.Data.Grant.ID != createReceipt.Data.Grant.ID || revokeReceipt.Data.Grant.RevokedAt == nil {
		t.Fatalf("revoke response grant = %#v", revokeReceipt.Data)
	}

	var revokedBy string
	var revokedAt *time.Time
	var revokedAuditEventID string
	if err := db.Pool.QueryRow(ctx, `
SELECT revoked_by_user_id, revoked_at, revoked_audit_event_id
FROM conversation_agent_action_grants WHERE id = $1`, createReceipt.Data.Grant.ID).
		Scan(&revokedBy, &revokedAt, &revokedAuditEventID); err != nil {
		t.Fatalf("read revoked grant = %v", err)
	}
	if revokedBy != subjectID || revokedAt == nil || revokedAuditEventID == "" {
		t.Fatalf("revoked grant record did not contain expected revocation fields: by=%s revokedAt=%v audit=%s", revokedBy, revokedAt, revokedAuditEventID)
	}

	// Second revoke with the same exact authority/idempotency must replay the
	// same logical revoke and cannot mint a second side effect.
	revokeReplayRequest := httptest.NewRequest(http.MethodDelete, "/api/v1/conversations/"+conversationID+"/agent-action-grants/"+createReceipt.Data.Grant.ID, bytes.NewReader(wireRevokeBody))
	signConversationRequest(t, revokeReplayRequest, wireRevokeBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	revokeReplayResponse := performRequest(router, revokeReplayRequest)
	if revokeReplayResponse.Code != http.StatusOK {
		t.Fatalf("replay revoke status/body = %d/%s", revokeReplayResponse.Code, revokeReplayResponse.Body.String())
	}
	var revokeReplayReceipt ownerGrantHTTPResponse
	if err := json.Unmarshal(revokeReplayResponse.Body.Bytes(), &revokeReplayReceipt); err != nil {
		t.Fatalf("replay revoke unmarshal error = %v", err)
	}
	if !revokeReplayReceipt.Data.Replayed || revokeReplayReceipt.Data.Grant.ID != createReceipt.Data.Grant.ID {
		t.Fatalf("expected replayed revoke, got %#v", revokeReplayReceipt.Data)
	}

	// After revoke, the active authorization check must fail before any durable
	// ticket effect is written.
	_, err = service.CreateTicketOperation(ctx, conversation.CreateTicketInput{
		OrgID:                    orgID,
		ConversationID:           conversationID,
		WorkType:                 "customer_case",
		ActorUserID:              subjectID,
		IdempotencyKey:           "ticket-op-after-revoke-" + conversationID,
		AgentActionAuthorization: ownerEffectAuthorization,
	})
	if !errors.Is(err, conversation.ErrForbidden) {
		t.Fatalf("post-revoke ticket operation = %v, want ErrForbidden", err)
	}

	// The service repository created by this proof must reflect expected live
	// storage when reads are run with an independent SQL query.
	var ticketCount int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_tickets WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID).Scan(&ticketCount); err != nil {
		t.Fatalf("read tickets = %v", err)
	}
	if ticketCount != 1 {
		t.Fatalf("ticket count = %d, want 1", ticketCount)
	}

}

func TestLiveOwnerGrantCreateRejectsExpiredDecision(t *testing.T) {
	orgID := fmt.Sprintf("proof-org-expired-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-conversation-expired-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-inbox-expired-%d", time.Now().UnixNano()+29)

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-proof-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, service)
	handler.SetOwnerGrantDecisionVerifier(verifier)
	router := newRouter(handler, testVerifier(t))

	createDecision := agentTicketGrantDecision("create", "")
	createDecision.OrgID = orgID
	createDecision.ConversationID = conversationID
	createDecision.IdempotencyKey = "owner-grant-create-expired-" + conversationID
	now := time.Now().UTC()
	createDecision.IssuedAt = now.Add(-2 * time.Minute)
	createDecision.ExpiresAt = now.Add(-time.Minute)
	createBody := ownerGrantBody{
		IdempotencyKey:       createDecision.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", createDecision),
	}
	wireCreateBody, err := json.Marshal(createBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}

	createRequest := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/"+conversationID+"/agent-action-grants", bytes.NewReader(wireCreateBody))
	signConversationRequest(t, createRequest, wireCreateBody, "verevon-gateway", testGatewaySecret, "user-expired", orgID, "owner")
	createResponse := performRequest(router, createRequest)
	if createResponse.Code != http.StatusForbidden {
		t.Fatalf("expired decision status/body = %d/%s", createResponse.Code, createResponse.Body.String())
	}

	var count int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_agent_action_grants WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID).Scan(&count); err != nil {
		t.Fatalf("read grants = %v", err)
	}
	if count != 0 {
		t.Fatalf("expired grant should not be persisted; count=%d", count)
	}
}

func TestLiveOwnerGrantCreateRejectsConversationMismatch(t *testing.T) {
	orgID := fmt.Sprintf("proof-org-mismatch-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-conversation-mismatch-%d", time.Now().UnixNano()+13)
	otherConversationID := conversationID + "-other"
	inboxID := fmt.Sprintf("proof-inbox-mismatch-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-user-%d", time.Now().UnixNano()+41)
	recipientAudienceRef := fmt.Sprintf("audience-%s", orgID)
	recipientAudienceHash := fmt.Sprintf("hash-%d", time.Now().UnixNano()+53)

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-proof-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, service)
	handler.SetOwnerGrantDecisionVerifier(verifier)
	router := newRouter(handler, testVerifier(t))

	createDecision := agentTicketGrantDecision("create", "")
	createDecision.OrgID = orgID
	createDecision.ConversationID = conversationID
	createDecision.SpaceRef = fmt.Sprintf("space-%s", orgID)
	createDecision.SubjectID = subjectID
	createDecision.RecipientAudienceRef = recipientAudienceRef
	createDecision.RecipientAudienceHash = recipientAudienceHash
	createDecision.IdempotencyKey = "owner-grant-create-mismatch-" + conversationID
	createBody := ownerGrantBody{
		IdempotencyKey:       createDecision.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", createDecision),
	}
	wireCreateBody, err := json.Marshal(createBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}

	// Use a foreign path that does not match the signed decision's conversation_id.
	createRequest := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/"+otherConversationID+"/agent-action-grants", bytes.NewReader(wireCreateBody))
	signConversationRequest(t, createRequest, wireCreateBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	createResponse := performRequest(router, createRequest)
	if createResponse.Code != http.StatusForbidden {
		t.Fatalf("conversation mismatch decision status/body = %d/%s", createResponse.Code, createResponse.Body.String())
	}

	var count int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_agent_action_grants WHERE org_id = $1 AND conversation_id IN ($2, $3)`, orgID, conversationID, otherConversationID).Scan(&count); err != nil {
		t.Fatalf("read grants = %v", err)
	}
	if count != 0 {
		t.Fatalf("mismatched path should not write any grants; count=%d", count)
	}
}

func TestLiveOwnerGrantCreateRejectsForgedExpiredOrReboundTokens(t *testing.T) {
	orgID := fmt.Sprintf("proof-org-token-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-conversation-token-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-inbox-token-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-user-token-%d", time.Now().UnixNano()+41)
	spaceRef := fmt.Sprintf("space-%s", orgID)
	recipientAudienceRef := fmt.Sprintf("audience-%s", orgID)
	recipientAudienceHash := fmt.Sprintf("hash-%d", time.Now().UnixNano()+53)

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-proof-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, service)
	handler.SetOwnerGrantDecisionVerifier(verifier)
	router := newRouter(handler, testVerifier(t))

	now := time.Now().UTC()
	baseDecision := agentTicketGrantDecision("create", "")
	baseDecision.OrgID = orgID
	baseDecision.ConversationID = conversationID
	baseDecision.SpaceRef = spaceRef
	baseDecision.SubjectID = subjectID
	baseDecision.RecipientAudienceRef = recipientAudienceRef
	baseDecision.RecipientAudienceHash = recipientAudienceHash
	baseDecision.IssuedAt = now.Add(-time.Second)
	baseDecision.ExpiresAt = now.Add(time.Minute)

	makeRequest := func(t *testing.T, decision ownerGrantDecision, token, targetConversationID, tokenUserID, tokenOrgID string) *httptest.ResponseRecorder {
		t.Helper()
		body := ownerGrantBody{
			IdempotencyKey:       decision.IdempotencyKey,
			ControlDecisionToken: token,
		}
		wire, err := json.Marshal(body)
		if err != nil {
			t.Fatalf("Marshal() error = %v", err)
		}
		request := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/"+targetConversationID+"/agent-action-grants", bytes.NewReader(wire))
		signConversationRequest(t, request, wire, "verevon-gateway", testGatewaySecret, tokenUserID, tokenOrgID, "owner")
		return performRequest(router, request)
	}

	forgedToken := signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", baseDecision) + ".forged"
	response := makeRequest(t, baseDecision, forgedToken, conversationID, subjectID, orgID)
	if response.Code != http.StatusForbidden {
		t.Fatalf("forged token status/body = %d/%s", response.Code, response.Body.String())
	}

	expiredDecision := baseDecision
	expiredDecision.IdempotencyKey = "owner-grant-expired-" + conversationID
	expiredDecision.IssuedAt = now.Add(-2 * time.Minute)
	expiredDecision.ExpiresAt = now.Add(-time.Minute)
	response = makeRequest(t, expiredDecision, signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", expiredDecision), conversationID, subjectID, orgID)
	if response.Code != http.StatusForbidden {
		t.Fatalf("expired decision status/body = %d/%s", response.Code, response.Body.String())
	}

	wrongOrgDecision := baseDecision
	wrongOrgDecision.IdempotencyKey = "owner-grant-wrong-org-" + conversationID
	wrongOrgDecision.OrgID = "other-org"
	response = makeRequest(t, wrongOrgDecision, signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", wrongOrgDecision), conversationID, subjectID, orgID)
	if response.Code != http.StatusForbidden {
		t.Fatalf("wrong-org decision status/body = %d/%s", response.Code, response.Body.String())
	}

	wrongUserDecision := baseDecision
	wrongUserDecision.IdempotencyKey = "owner-grant-wrong-user-" + conversationID
	wrongUserDecision.SubjectID = "other-user"
	response = makeRequest(t, wrongUserDecision, signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", wrongUserDecision), conversationID, subjectID, orgID)
	if response.Code != http.StatusForbidden {
		t.Fatalf("wrong-user decision status/body = %d/%s", response.Code, response.Body.String())
	}

	wrongConversationDecision := baseDecision
	wrongConversationDecision.IdempotencyKey = "owner-grant-wrong-conversation-" + conversationID
	wrongConversationDecision.ConversationID = conversationID + "-other"
	response = makeRequest(t, wrongConversationDecision, signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", wrongConversationDecision), conversationID, subjectID, orgID)
	if response.Code != http.StatusForbidden {
		t.Fatalf("wrong-conversation decision status/body = %d/%s", response.Code, response.Body.String())
	}

	var count int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_agent_action_grants WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID).Scan(&count); err != nil {
		t.Fatalf("read grants = %v", err)
	}
	if count != 0 {
		t.Fatalf("unauthorized owner grant decisions should not persist; count=%d", count)
	}
}

func TestLiveOwnerGrantRevocationRevisionHardening(t *testing.T) {
	orgID := fmt.Sprintf("proof-org-rev-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-conversation-rev-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-inbox-rev-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-user-rev-%d", time.Now().UnixNano()+41)
	spaceRef := fmt.Sprintf("space-%s", orgID)
	recipientAudienceRef := fmt.Sprintf("audience-%s", orgID)
	recipientAudienceHash := fmt.Sprintf("hash-%d", time.Now().UnixNano()+53)

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-proof-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, service)
	handler.SetOwnerGrantDecisionVerifier(verifier)
	router := newRouter(handler, testVerifier(t))

	createDecision := agentTicketGrantDecision("create", "")
	createDecision.OrgID = orgID
	createDecision.ConversationID = conversationID
	createDecision.SpaceRef = spaceRef
	createDecision.SubjectID = subjectID
	createDecision.RecipientAudienceRef = recipientAudienceRef
	createDecision.RecipientAudienceHash = recipientAudienceHash
	createDecision.IdempotencyKey = "owner-grant-create-rev-" + conversationID

	createBody := ownerGrantBody{
		IdempotencyKey:       createDecision.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", createDecision),
	}
	wireCreateBody, err := json.Marshal(createBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	createRequest := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/"+conversationID+"/agent-action-grants", bytes.NewReader(wireCreateBody))
	signConversationRequest(t, createRequest, wireCreateBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	createResponse := performRequest(router, createRequest)
	if createResponse.Code != http.StatusCreated {
		t.Fatalf("create status/body = %d/%s", createResponse.Code, createResponse.Body.String())
	}

	var createReceipt ownerGrantHTTPResponse
	if err := json.Unmarshal(createResponse.Body.Bytes(), &createReceipt); err != nil {
		t.Fatalf("create response unmarshal error = %v", err)
	}

	baseAuth := conversation.AgentTicketActionAuthorization{
		DecisionRef:               createDecision.DecisionRef,
		SpaceRef:                  createDecision.SpaceRef,
		SubjectID:                 createDecision.SubjectID,
		RecipientAudienceRef:      createDecision.RecipientAudienceRef,
		RecipientAudienceHash:     createDecision.RecipientAudienceHash,
		RecipientAudienceRevision: createDecision.RecipientAudienceRevision,
		PrivacyPolicyRef:          createDecision.PrivacyPolicyRef,
		AuthorityRevision:         createDecision.AuthorityRevision,
	}
	// A valid Control token for another Space is not enough to create a
	// ticket. The actual effect boundary intersects every run-action claim
	// with the durable owner grant for this exact Space. Exercise that through
	// the signed Execution Core HTTP path, rather than only mutating a Go
	// input, so this is a real wrong-Space token proof.
	runPublicKey, runPrivateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey(run action) error = %v", err)
	}
	runVerifier, err := NewRunActionDecisionVerifier("control-run-proof-key", base64.RawStdEncoding.EncodeToString(runPublicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	handler.SetRunActionDecisionVerifier(runVerifier)
	// This suite proves the durable owner-grant intersection. The separate
	// current-Control HTTP validator has its own exact request/denial tests;
	// keep this local disposable-Postgres proof focused on the row-level grant
	// constraints it is designed to exercise.
	handler.SetRunActionAuthorityValidator(runActionAuthorityValidatorFunc(func(context.Context, runActionDecision) error { return nil }))
	runRouter := agentTicketTestRouter(t, handler)
	wrongSpaceBody := agentTicketCreateBody{
		RunID: "run-wrong-space-" + conversationID, ConversationID: conversationID,
		IdempotencyKey: "ticket-op-wrong-space-" + conversationID, WorkType: "customer_case",
	}
	wrongSpaceDecision := agentTicketTestDecision(wrongSpaceBody)
	wrongSpaceDecision.RunID = wrongSpaceBody.RunID
	wrongSpaceDecision.OrgID = orgID
	wrongSpaceDecision.SpaceRef = "space-wrong"
	wrongSpaceDecision.SubjectID = subjectID
	wrongSpaceDecision.IdempotencyKey = wrongSpaceBody.IdempotencyKey
	wrongSpaceDecision.RecipientAudienceRef = recipientAudienceRef
	wrongSpaceDecision.RecipientAudienceHash = recipientAudienceHash
	wrongSpaceDecision.RecipientAudienceRevision = createDecision.RecipientAudienceRevision
	wrongSpaceDecision.PrivacyPolicyRef = createDecision.PrivacyPolicyRef
	wrongSpaceDecision.AuthorityRevision = createDecision.AuthorityRevision
	wrongSpaceDecision.PayloadDigest = agentTicketPayloadDigest(wrongSpaceDecision.RunID, wrongSpaceDecision.OrgID, wrongSpaceBody)
	wrongSpaceBody.ControlDecisionToken = signRunActionDecisionForTest(t, runPrivateKey, "control-run-proof-key", wrongSpaceDecision)
	wireWrongSpaceEffect, err := json.Marshal(wrongSpaceBody)
	if err != nil {
		t.Fatalf("Marshal(wrong Space effect) error = %v", err)
	}
	wrongSpaceEffectRequest := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireWrongSpaceEffect))
	signConversationRequest(t, wrongSpaceEffectRequest, wireWrongSpaceEffect, "execution-core", agentTicketExecutionSecret, "", "", "")
	wrongSpaceEffectResponse := performRequest(runRouter, wrongSpaceEffectRequest)
	if wrongSpaceEffectResponse.Code != http.StatusForbidden {
		t.Fatalf("wrong-Space effect token status/body = %d/%s", wrongSpaceEffectResponse.Code, wrongSpaceEffectResponse.Body.String())
	}
	for name, mutated := range map[string]conversation.AgentTicketActionAuthorization{
		"audience ref mismatch":       {DecisionRef: baseAuth.DecisionRef, SpaceRef: baseAuth.SpaceRef, SubjectID: baseAuth.SubjectID, RecipientAudienceRef: "audience-other", RecipientAudienceHash: baseAuth.RecipientAudienceHash, RecipientAudienceRevision: baseAuth.RecipientAudienceRevision, PrivacyPolicyRef: baseAuth.PrivacyPolicyRef, AuthorityRevision: baseAuth.AuthorityRevision},
		"audience hash mismatch":      {DecisionRef: baseAuth.DecisionRef, SpaceRef: baseAuth.SpaceRef, SubjectID: baseAuth.SubjectID, RecipientAudienceRef: baseAuth.RecipientAudienceRef, RecipientAudienceHash: "hash-other", RecipientAudienceRevision: baseAuth.RecipientAudienceRevision, PrivacyPolicyRef: baseAuth.PrivacyPolicyRef, AuthorityRevision: baseAuth.AuthorityRevision},
		"audience revision mismatch":  {DecisionRef: baseAuth.DecisionRef, SpaceRef: baseAuth.SpaceRef, SubjectID: baseAuth.SubjectID, RecipientAudienceRef: baseAuth.RecipientAudienceRef, RecipientAudienceHash: baseAuth.RecipientAudienceHash, RecipientAudienceRevision: baseAuth.RecipientAudienceRevision + 9, PrivacyPolicyRef: baseAuth.PrivacyPolicyRef, AuthorityRevision: baseAuth.AuthorityRevision},
		"privacy revision mismatch":   {DecisionRef: baseAuth.DecisionRef, SpaceRef: baseAuth.SpaceRef, SubjectID: baseAuth.SubjectID, RecipientAudienceRef: baseAuth.RecipientAudienceRef, RecipientAudienceHash: baseAuth.RecipientAudienceHash, RecipientAudienceRevision: baseAuth.RecipientAudienceRevision, PrivacyPolicyRef: "privacy-other", AuthorityRevision: baseAuth.AuthorityRevision},
		"authority revision mismatch": {DecisionRef: baseAuth.DecisionRef, SpaceRef: baseAuth.SpaceRef, SubjectID: baseAuth.SubjectID, RecipientAudienceRef: baseAuth.RecipientAudienceRef, RecipientAudienceHash: baseAuth.RecipientAudienceHash, RecipientAudienceRevision: baseAuth.RecipientAudienceRevision, PrivacyPolicyRef: baseAuth.PrivacyPolicyRef, AuthorityRevision: baseAuth.AuthorityRevision + 9},
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := service.CreateTicketOperation(ctx, conversation.CreateTicketInput{
				OrgID:                    orgID,
				ConversationID:           conversationID,
				WorkType:                 "customer_case",
				ActorUserID:              subjectID,
				IdempotencyKey:           "ticket-op-allowed-mismatch-" + conversationID + "-" + name,
				AgentActionAuthorization: &mutated,
			}); !errors.Is(err, conversation.ErrForbidden) {
				t.Fatalf("ticket effect with %s = %v, want ErrForbidden", name, err)
			}
		})
	}

	var ticketCount int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_tickets WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID).Scan(&ticketCount); err != nil {
		t.Fatalf("read tickets = %v", err)
	}
	if ticketCount != 0 {
		t.Fatalf("mutated authorization should not authorise an operation before revoke")
	}

	wrongSpaceRevoke := agentTicketGrantDecision("revoke", createReceipt.Data.Grant.ID)
	wrongSpaceRevoke.OrgID = orgID
	wrongSpaceRevoke.ConversationID = conversationID
	wrongSpaceRevoke.SpaceRef = "space-wrong"
	wrongSpaceRevoke.SubjectID = subjectID
	wrongSpaceRevoke.IdempotencyKey = "owner-grant-revoke-wrong-space-" + conversationID
	wrongSpaceWire := ownerGrantBody{
		IdempotencyKey:       wrongSpaceRevoke.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", wrongSpaceRevoke),
	}
	wireWrongSpace, err := json.Marshal(wrongSpaceWire)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	wrongSpaceRevokeRequest := httptest.NewRequest(http.MethodDelete, "/api/v1/conversations/"+conversationID+"/agent-action-grants/"+createReceipt.Data.Grant.ID, bytes.NewReader(wireWrongSpace))
	signConversationRequest(t, wrongSpaceRevokeRequest, wireWrongSpace, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	wrongSpaceRevokeResponse := performRequest(router, wrongSpaceRevokeRequest)
	if wrongSpaceRevokeResponse.Code != http.StatusForbidden {
		t.Fatalf("wrong-space revoke status/body = %d/%s", wrongSpaceRevokeResponse.Code, wrongSpaceRevokeResponse.Body.String())
	}

	revokeDecision := agentTicketGrantDecision("revoke", createReceipt.Data.Grant.ID)
	revokeDecision.OrgID = orgID
	revokeDecision.ConversationID = conversationID
	revokeDecision.SpaceRef = createDecision.SpaceRef
	revokeDecision.SubjectID = subjectID
	revokeDecision.IdempotencyKey = "owner-grant-revoke-ok-" + conversationID
	revokeDecision.RecipientAudienceRef = "audience-other"
	revokeDecision.RecipientAudienceHash = "hash-other"
	revokeDecision.RecipientAudienceRevision = 99
	revokeDecision.PrivacyPolicyRef = "privacy-other"
	revokeDecision.AuthorityRevision = 99
	revokeBody := ownerGrantBody{
		IdempotencyKey:       revokeDecision.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", revokeDecision),
	}
	wireRevokeBody, err := json.Marshal(revokeBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	revokeRequest := httptest.NewRequest(http.MethodDelete, "/api/v1/conversations/"+conversationID+"/agent-action-grants/"+createReceipt.Data.Grant.ID, bytes.NewReader(wireRevokeBody))
	signConversationRequest(t, revokeRequest, wireRevokeBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	revokeResponse := performRequest(router, revokeRequest)
	if revokeResponse.Code != http.StatusOK {
		t.Fatalf("revoke status/body = %d/%s", revokeResponse.Code, revokeResponse.Body.String())
	}
	var revokeReceipt ownerGrantHTTPResponse
	if err := json.Unmarshal(revokeResponse.Body.Bytes(), &revokeReceipt); err != nil {
		t.Fatalf("unmarshal revoke response = %v", err)
	}
	if revokeReceipt.Data.Grant.ID != createReceipt.Data.Grant.ID || revokeReceipt.Data.Status != "revoked" || revokeReceipt.Data.Grant.RevokedAt == nil {
		t.Fatalf("revoke receipt = %#v", revokeReceipt.Data)
	}

	var revokedBy string
	var revokedAt *time.Time
	var revokedAuditEventID string
	if err := db.Pool.QueryRow(ctx, `
SELECT revoked_by_user_id, revoked_at, revoked_audit_event_id
FROM conversation_agent_action_grants WHERE id = $1`, createReceipt.Data.Grant.ID).
		Scan(&revokedBy, &revokedAt, &revokedAuditEventID); err != nil {
		t.Fatalf("read revoked grant = %v", err)
	}
	if revokedBy != subjectID || revokedAt == nil || revokedAuditEventID == "" {
		t.Fatalf("revoke did not persist expected fields: by=%s at=%v audit=%s", revokedBy, revokedAt, revokedAuditEventID)
	}
}

func TestLiveOwnerGrantRevokeAndEffectRace(t *testing.T) {
	orgID := fmt.Sprintf("proof-org-race-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-conversation-race-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-inbox-race-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-user-race-%d", time.Now().UnixNano()+41)
	spaceRef := fmt.Sprintf("space-%s", orgID)
	recipientAudienceRef := fmt.Sprintf("audience-%s", orgID)
	recipientAudienceHash := fmt.Sprintf("hash-%d", time.Now().UnixNano()+53)

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	verifier, err := NewOwnerGrantDecisionVerifier("control-proof-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewOwnerGrantDecisionVerifier() error = %v", err)
	}
	handler := NewHandler(nil, service)
	handler.SetOwnerGrantDecisionVerifier(verifier)
	router := newRouter(handler, testVerifier(t))

	createDecision := agentTicketGrantDecision("create", "")
	createDecision.OrgID = orgID
	createDecision.ConversationID = conversationID
	createDecision.SpaceRef = spaceRef
	createDecision.SubjectID = subjectID
	createDecision.RecipientAudienceRef = recipientAudienceRef
	createDecision.RecipientAudienceHash = recipientAudienceHash
	createDecision.IdempotencyKey = "owner-grant-race-create-" + conversationID

	createBody := ownerGrantBody{IdempotencyKey: createDecision.IdempotencyKey, ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", createDecision)}
	wireCreateBody, err := json.Marshal(createBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}
	createRequest := httptest.NewRequest(http.MethodPost, "/api/v1/conversations/"+conversationID+"/agent-action-grants", bytes.NewReader(wireCreateBody))
	signConversationRequest(t, createRequest, wireCreateBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
	createResponse := performRequest(router, createRequest)
	if createResponse.Code != http.StatusCreated {
		t.Fatalf("create status/body = %d/%s", createResponse.Code, createResponse.Body.String())
	}
	var createReceipt ownerGrantHTTPResponse
	if err := json.Unmarshal(createResponse.Body.Bytes(), &createReceipt); err != nil {
		t.Fatalf("create response unmarshal error = %v", err)
	}

	baseAuth := &conversation.AgentTicketActionAuthorization{
		DecisionRef:               createDecision.DecisionRef,
		SpaceRef:                  createDecision.SpaceRef,
		SubjectID:                 createDecision.SubjectID,
		RecipientAudienceRef:      createDecision.RecipientAudienceRef,
		RecipientAudienceHash:     createDecision.RecipientAudienceHash,
		RecipientAudienceRevision: createDecision.RecipientAudienceRevision,
		PrivacyPolicyRef:          createDecision.PrivacyPolicyRef,
		AuthorityRevision:         createDecision.AuthorityRevision,
	}
	revokeDecision := agentTicketGrantDecision("revoke", createReceipt.Data.Grant.ID)
	revokeDecision.OrgID = orgID
	revokeDecision.ConversationID = conversationID
	revokeDecision.SpaceRef = spaceRef
	revokeDecision.SubjectID = subjectID
	revokeDecision.IdempotencyKey = "owner-grant-race-revoke-" + conversationID
	revokeBody := ownerGrantBody{
		IdempotencyKey:       revokeDecision.IdempotencyKey,
		ControlDecisionToken: signOwnerGrantDecisionForTest(t, privateKey, "control-proof-key", revokeDecision),
	}
	wireRevokeBody, err := json.Marshal(revokeBody)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}

	type raceResult struct {
		ticketErr error
		revokeErr error
	}
	raceCh := make(chan raceResult, 2)
	var wg sync.WaitGroup
	wg.Add(2)

	go func() {
		defer wg.Done()
		_, err := service.CreateTicketOperation(ctx, conversation.CreateTicketInput{
			OrgID:                    orgID,
			ConversationID:           conversationID,
			WorkType:                 "customer_case",
			ActorUserID:              subjectID,
			IdempotencyKey:           "ticket-op-race-" + conversationID,
			AgentActionAuthorization: baseAuth,
		})
		raceCh <- raceResult{ticketErr: err}
	}()

	go func() {
		defer wg.Done()
		revokeRequest := httptest.NewRequest(http.MethodDelete, "/api/v1/conversations/"+conversationID+"/agent-action-grants/"+createReceipt.Data.Grant.ID, bytes.NewReader(wireRevokeBody))
		signConversationRequest(t, revokeRequest, wireRevokeBody, "verevon-gateway", testGatewaySecret, subjectID, orgID, "owner")
		response := performRequest(router, revokeRequest)
		if response.Code != http.StatusOK {
			raceCh <- raceResult{revokeErr: errors.New(response.Body.String())}
			return
		}
		raceCh <- raceResult{}
	}()
	wg.Wait()
	result1 := <-raceCh
	result2 := <-raceCh
	if result1.revokeErr != nil && result2.revokeErr != nil {
		t.Fatalf("revoke race status/body = %s", result1.revokeErr)
	}
	if result1.ticketErr != nil && result2.ticketErr != nil {
		if !errors.Is(result1.ticketErr, conversation.ErrForbidden) && !errors.Is(result2.ticketErr, conversation.ErrForbidden) {
			t.Fatalf("ticket operation race returned unexpected error = %v, %v", result1.ticketErr, result2.ticketErr)
		}
	} else if result1.ticketErr != nil && !errors.Is(result1.ticketErr, conversation.ErrForbidden) {
		t.Fatalf("ticket operation race returned unexpected error = %v", result1.ticketErr)
	} else if result2.ticketErr != nil && !errors.Is(result2.ticketErr, conversation.ErrForbidden) {
		t.Fatalf("ticket operation race returned unexpected error = %v", result2.ticketErr)
	}

	var revokedAt *time.Time
	if err := db.Pool.QueryRow(ctx, `
SELECT revoked_at FROM conversation_agent_action_grants WHERE id = $1`, createReceipt.Data.Grant.ID).Scan(&revokedAt); err != nil {
		t.Fatalf("read revoked grant = %v", err)
	}
	if revokedAt == nil {
		t.Fatalf("race revoke did not mark revoked_at")
	}

	var ticketCount int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_tickets WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID).Scan(&ticketCount); err != nil {
		t.Fatalf("read tickets = %v", err)
	}
	if ticketCount != 0 && ticketCount != 1 {
		t.Fatalf("ticket race should write at most one ticket, got %d", ticketCount)
	}
}

func TestLiveOwnerGrantCreateRevokeIdempotentAndReplayed(t *testing.T) {
	orgID := fmt.Sprintf("proof-org-idem-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-conversation-idem-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-inbox-idem-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-user-idem-%d", time.Now().UnixNano()+41)
	spaceRef := fmt.Sprintf("space-%s", orgID)
	recipientAudienceRef := fmt.Sprintf("audience-%s", orgID)
	recipientAudienceHash := fmt.Sprintf("hash-%d", time.Now().UnixNano()+53)
	privacyPolicyRef := "privacy-idem"
	now := time.Now().UTC()

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()

	createDecision := agentTicketGrantDecision("create", "")
	createDecision.OrgID = orgID
	createDecision.ConversationID = conversationID
	createDecision.SpaceRef = spaceRef
	createDecision.SubjectID = subjectID
	createDecision.RecipientAudienceRef = recipientAudienceRef
	createDecision.RecipientAudienceHash = recipientAudienceHash
	createDecision.PrivacyPolicyRef = privacyPolicyRef
	createDecision.IssuedAt = now.Add(-time.Second)
	createDecision.ExpiresAt = now.Add(time.Minute)
	createDecision.IdempotencyKey = "owner-grant-idem-" + conversationID
	createInput := conversation.CreateAgentTicketActionGrantInput{
		OrgID: createDecision.OrgID, ConversationID: createDecision.ConversationID, ActionID: createDecision.ActionID,
		SpaceRef: createDecision.SpaceRef, SubjectID: createDecision.SubjectID,
		RecipientAudienceRef: createDecision.RecipientAudienceRef, RecipientAudienceHash: createDecision.RecipientAudienceHash,
		RecipientAudienceRevision: createDecision.RecipientAudienceRevision, PrivacyPolicyRef: createDecision.PrivacyPolicyRef,
		AuthorityRevision: createDecision.AuthorityRevision, CreatedByUserID: subjectID,
		IdempotencyKey: createDecision.IdempotencyKey, RequestSHA256: ownerGrantRequestSHA256(createDecision), ControlDecisionRef: createDecision.DecisionRef,
	}

	var firstReceipt *conversation.AgentTicketActionGrantReceipt
	var secondReceipt *conversation.AgentTicketActionGrantReceipt
	type createResult struct {
		receipt *conversation.AgentTicketActionGrantReceipt
		err     error
	}
	var createWG sync.WaitGroup
	createResultCh := make(chan createResult, 2)
	createWG.Add(2)
	go func() {
		defer createWG.Done()
		receipt, err := service.CreateAgentTicketActionGrant(context.Background(), createInput)
		createResultCh <- createResult{receipt: receipt, err: err}
	}()
	go func() {
		defer createWG.Done()
		receipt, err := service.CreateAgentTicketActionGrant(context.Background(), createInput)
		createResultCh <- createResult{receipt: receipt, err: err}
	}()
	createWG.Wait()

	firstResult := <-createResultCh
	secondResult := <-createResultCh
	close(createResultCh)
	if firstResult.err != nil {
		t.Fatalf("concurrent create first = %v", firstResult.err)
	}
	if secondResult.err != nil {
		t.Fatalf("concurrent create second = %v", secondResult.err)
	}
	if firstResult.receipt == nil || secondResult.receipt == nil {
		t.Fatal("concurrent create did not return both receipts")
	}
	firstReceipt = firstResult.receipt
	secondReceipt = secondResult.receipt
	if firstReceipt.Grant.ID != secondReceipt.Grant.ID {
		t.Fatalf("expected same grant id from replayed create; got %q and %q", firstReceipt.Grant.ID, secondReceipt.Grant.ID)
	}
	if !(firstReceipt.Replayed || secondReceipt.Replayed) {
		t.Fatalf("expected at least one replayed receipt, got %#v %#v", firstReceipt.Replayed, secondReceipt.Replayed)
	}

	var grantCount int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_agent_action_grants WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID).Scan(&grantCount); err != nil {
		t.Fatalf("read grants = %v", err)
	}
	if grantCount != 1 {
		t.Fatalf("grants count = %d, want 1", grantCount)
	}

	revokeDecision := agentTicketGrantDecision("revoke", firstReceipt.Grant.ID)
	revokeDecision.OrgID = orgID
	revokeDecision.ConversationID = conversationID
	revokeDecision.SpaceRef = spaceRef
	revokeDecision.SubjectID = subjectID
	revokeDecision.IdempotencyKey = "owner-grant-idem-revoke-" + conversationID
	revokeDecision.IssuedAt = now.Add(-time.Second)
	revokeDecision.ExpiresAt = now.Add(time.Minute)

	revokeInput := conversation.RevokeAgentTicketActionGrantInput{
		OrgID: revokeDecision.OrgID, ConversationID: revokeDecision.ConversationID, GrantID: revokeDecision.GrantID,
		SpaceRef: revokeDecision.SpaceRef, SubjectID: revokeDecision.SubjectID, RevokedByUserID: subjectID,
		IdempotencyKey: revokeDecision.IdempotencyKey, RequestSHA256: ownerGrantRequestSHA256(revokeDecision), ControlDecisionRef: revokeDecision.DecisionRef,
	}
	firstRevokeReceipt, firstRevokeErr := service.RevokeAgentTicketActionGrant(ctx, revokeInput)
	if firstRevokeErr != nil {
		t.Fatalf("revoke first = %v", firstRevokeErr)
	}
	secondRevokeReceipt, secondRevokeErr := service.RevokeAgentTicketActionGrant(ctx, revokeInput)
	if secondRevokeErr != nil {
		t.Fatalf("revoke replay = %v", secondRevokeErr)
	}
	if firstRevokeReceipt.Grant.ID != secondRevokeReceipt.Grant.ID {
		t.Fatalf("expected same revoked grant; got %q and %q", firstRevokeReceipt.Grant.ID, secondRevokeReceipt.Grant.ID)
	}
	if !secondRevokeReceipt.Replayed {
		t.Fatalf("expected second revoke to be replayed: %#v %#v", firstRevokeReceipt.Replayed, secondRevokeReceipt.Replayed)
	}
}
