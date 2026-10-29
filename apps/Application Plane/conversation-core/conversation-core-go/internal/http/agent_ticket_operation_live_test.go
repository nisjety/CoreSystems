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
	"github.com/jackc/pgx/v5/pgxpool"
)

// TestLiveAgentTicketOperationIntentUnknownLifecycle proves the owner-side
// intent survives the Control commit boundary without retaining ticket
// content. It intentionally never creates a ticket: an ambiguous owner
// transaction must be represented by a durable unknown receipt, not a missing
// row or an implicit retry permission.
func TestLiveAgentTicketOperationIntentUnknownLifecycle(t *testing.T) {
	orgID := fmt.Sprintf("proof-intent-org-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-intent-conversation-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-intent-inbox-%d", time.Now().UnixNano()+29)
	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()
	input := conversation.TicketOperationIntentInput{
		OperationID: "ticketop-proof-intent-unknown", OrgID: orgID, IdempotencyKey: "intent-unknown-001",
		ActionID: "tickets.create", ActorUserID: "user-intent-proof", ConversationID: conversationID,
		RequestSHA256:    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		ActionSchemaHash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		PayloadDigest:    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		DecisionRef:      "decision-intent-proof", GrantRef: "grant-intent-proof",
	}
	first, err := service.BeginAgentTicketOperationIntent(ctx, input)
	if err != nil || first.Status != "pending_control_commit" {
		t.Fatalf("begin intent = %#v, err=%v", first, err)
	}
	bound, err := service.BindAgentTicketOperationReservation(ctx, conversation.TicketOperationReservationInput{
		TicketOperationIntentInput: input, ControlReservationID: "reservation-intent-proof",
	})
	if err != nil || bound.Status != "reserved" {
		t.Fatalf("bind intent = %#v, err=%v", bound, err)
	}
	if err := service.MarkAgentTicketOperationUnknown(ctx, conversation.TicketOperationOutcomeInput{
		OrgID: orgID, OperationID: input.OperationID, IdempotencyKey: input.IdempotencyKey,
		ControlReservationID: "reservation-intent-proof", TerminalReason: "owner connection lost after submit",
	}); err != nil {
		t.Fatalf("mark unknown = %v", err)
	}
	receipt, err := service.GetTicketOperation(ctx, orgID, input.ActorUserID, input.IdempotencyKey)
	if err != nil || receipt.Status != "unknown" || receipt.Ticket != nil || receipt.TerminalReason == "" {
		t.Fatalf("unknown receipt = %#v, err=%v", receipt, err)
	}
	replay, err := service.BeginAgentTicketOperationIntent(ctx, input)
	if err != nil || replay.Status != "unknown" || !replay.Replayed {
		t.Fatalf("unknown replay = %#v, err=%v", replay, err)
	}
	changed := input
	changed.PayloadDigest = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
	if _, err := service.BeginAgentTicketOperationIntent(ctx, changed); err == nil || !errors.Is(err, conversation.ErrConflict) {
		t.Fatalf("changed payload replay = %v, want conflict", err)
	}
	if err := service.MarkAgentTicketOperationCancelled(ctx, conversation.TicketOperationOutcomeInput{
		OrgID: orgID, OperationID: input.OperationID, IdempotencyKey: input.IdempotencyKey,
		ControlReservationID: "reservation-intent-proof", TerminalReason: "late cancellation",
	}); err == nil || !errors.Is(err, conversation.ErrConflict) {
		t.Fatalf("late cancellation = %v, want conflict preserving unknown", err)
	}
	var status string
	var ticketID, auditEventID *string
	if err := db.Pool.QueryRow(ctx, `
SELECT status, ticket_id, audit_event_id
FROM conversation_ticket_operations
WHERE org_id = $1 AND operation_id = $2`, orgID, input.OperationID).Scan(&status, &ticketID, &auditEventID); err != nil {
		t.Fatalf("read unknown intent row = %v", err)
	}
	if status != "unknown" || ticketID != nil || auditEventID != nil {
		t.Fatalf("unknown intent row leaked owner effect: status=%s ticket=%v audit=%v", status, ticketID, auditEventID)
	}
}

// TestLiveAgentTicketOperationRevocationAtOwnerCommitBoundary proves the
// final owner transaction, rather than the earlier Control reservation, is
// the effect linearization point. The reservation coordinator is held after
// it reports commit entry; revocation then commits against the real
// Conversation database before the owner transaction is released. A ticket
// must not be written and the already-created operation intent must become a
// deterministic cancellation, never an apparent success or unknown result.
func TestLiveAgentTicketOperationRevocationAtOwnerCommitBoundary(t *testing.T) {
	orgID := fmt.Sprintf("proof-fence-org-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-fence-conversation-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-fence-inbox-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-fence-user-%d", time.Now().UnixNano()+41)
	spaceRef := "space-fence"
	audienceRef := "audience-fence"
	audienceHash := "audience-fence-hash"
	decisionRef := "decision-fence-" + conversationID
	body := agentTicketCreateBody{
		RunID: "run-fence-" + conversationID, ConversationID: conversationID,
		IdempotencyKey: "ticket-fence-" + conversationID, WorkType: "customer_case", Priority: "normal",
	}

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	ctx := context.Background()
	requestSHA := "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	createInput := conversation.CreateAgentTicketActionGrantInput{
		OrgID: orgID, ConversationID: conversationID, ActionID: "tickets.create", SpaceRef: spaceRef,
		SubjectID: subjectID, RecipientAudienceRef: audienceRef, RecipientAudienceHash: audienceHash,
		RecipientAudienceRevision: 1, PrivacyPolicyRef: "privacy-fence", AuthorityRevision: 1,
		CreatedByUserID: subjectID, IdempotencyKey: "grant-create-" + conversationID,
		RequestSHA256: requestSHA, ControlDecisionRef: decisionRef,
	}
	grantReceipt, err := service.CreateAgentTicketActionGrant(ctx, createInput)
	if err != nil || grantReceipt == nil || grantReceipt.Grant == nil {
		t.Fatalf("create owner grant = %#v, err=%v", grantReceipt, err)
	}

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-fence-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	decision := agentTicketTestDecision(body)
	decision.DecisionRef = decisionRef
	decision.OrgID = orgID
	decision.SpaceRef = spaceRef
	decision.SubjectID = subjectID
	decision.RecipientAudienceRef = audienceRef
	decision.RecipientAudienceHash = audienceHash
	decision.PrivacyPolicyRef = "privacy-fence"
	decision.AuthorityRevision = 1
	decision.IdempotencyKey = body.IdempotencyKey
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-fence-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}

	commitEntered := make(chan struct{})
	releaseCommit := make(chan struct{})
	var commitOnce sync.Once
	handler := NewHandler(nil, service)
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.SetOwnerEffectReservationCoordinator(readyOwnerEffectReservationCoordinator{
		commit: func(ctx context.Context, reservationID, token string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
			commitOnce.Do(func() { close(commitEntered) })
			select {
			case <-releaseCommit:
				return ownerEffectReservationReceipt{ReservationID: reservationID, OperationID: commitment.OperationID, Status: "committed"}, nil
			case <-ctx.Done():
				return ownerEffectReservationReceipt{}, ctx.Err()
			}
		},
	})
	router := agentTicketTestRouter(t, handler)
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	responseCh := make(chan *httptest.ResponseRecorder, 1)
	go func() { responseCh <- performRequest(router, request) }()

	select {
	case <-commitEntered:
	case <-time.After(5 * time.Second):
		t.Fatal("owner reservation commit was not entered")
	}

	revokeReceipt, err := service.RevokeAgentTicketActionGrant(ctx, conversation.RevokeAgentTicketActionGrantInput{
		OrgID: orgID, ConversationID: conversationID, GrantID: grantReceipt.Grant.ID,
		SpaceRef: spaceRef, SubjectID: subjectID, RevokedByUserID: subjectID,
		IdempotencyKey: "grant-revoke-" + conversationID, RequestSHA256: requestSHA, ControlDecisionRef: decisionRef,
	})
	if err != nil || revokeReceipt == nil || revokeReceipt.Grant == nil || revokeReceipt.Grant.RevokedAt == nil {
		t.Fatalf("revoke owner grant = %#v, err=%v", revokeReceipt, err)
	}
	close(releaseCommit)

	var response *httptest.ResponseRecorder
	select {
	case response = <-responseCh:
	case <-time.After(5 * time.Second):
		t.Fatal("owner operation did not finish after revocation")
	}
	if response.Code != http.StatusForbidden || !bytes.Contains(response.Body.Bytes(), []byte(`"code":"forbidden"`)) {
		t.Fatalf("status/body = %d/%s, want final owner fence denial", response.Code, response.Body.String())
	}

	operationID := conversation.TicketOperationID(orgID, body.IdempotencyKey)
	var operationStatus string
	if err := db.Pool.QueryRow(ctx, `
SELECT status FROM conversation_ticket_operations
WHERE org_id = $1 AND operation_id = $2`, orgID, operationID).Scan(&operationStatus); err != nil {
		t.Fatalf("read fenced operation = %v", err)
	}
	if operationStatus != "cancelled" {
		t.Fatalf("fenced operation status = %q, want cancelled", operationStatus)
	}
	var ticketCount int
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM conversation_tickets
WHERE org_id = $1 AND conversation_id = $2`, orgID, conversationID).Scan(&ticketCount); err != nil {
		t.Fatalf("read fenced tickets = %v", err)
	}
	if ticketCount != 0 {
		t.Fatalf("revoked owner commit wrote %d tickets", ticketCount)
	}
}

// TestLiveAgentTicketOperationRevocationAtOwnerCommitBoundaryAgainstControlPostgres
// extends the local fence proof across two real databases. The coordinator
// writes and commits the Control reservation in TEST_CONTROL_DATABASE_URL,
// then pauses before returning to Conversation Core. The test advances the
// Control authority revision and revokes the Conversation owner grant while
// the two transactions are independent; the final owner transaction must
// still deny the effect. This is a data-plane interleaving harness, not a
// substitute for a deployed authenticated Control HTTP journey.
func TestLiveAgentTicketOperationRevocationAtOwnerCommitBoundaryAgainstControlPostgres(t *testing.T) {
	controlDSN := os.Getenv("TEST_CONTROL_DATABASE_URL")
	if controlDSN == "" {
		t.Skip("TEST_CONTROL_DATABASE_URL not set; skipping cross-database owner fence proof")
	}
	ctx := context.Background()
	controlPool, err := pgxpool.New(ctx, controlDSN)
	if err != nil {
		t.Fatalf("connect Control Postgres: %v", err)
	}
	t.Cleanup(controlPool.Close)
	var migrationMarker string
	if err := controlPool.QueryRow(ctx, `SELECT to_regclass('public.space_owner_effect_reservations')`).Scan(&migrationMarker); err != nil || migrationMarker == "" {
		t.Skipf("Control migrations are not applied: %v", err)
	}

	orgID := fmt.Sprintf("proof-cross-db-org-%d", time.Now().UnixNano())
	conversationID := fmt.Sprintf("proof-cross-db-conversation-%d", time.Now().UnixNano()+13)
	inboxID := fmt.Sprintf("proof-cross-db-inbox-%d", time.Now().UnixNano()+29)
	subjectID := fmt.Sprintf("proof-cross-db-user-%d", time.Now().UnixNano()+41)
	spaceRef := "space-cross-db-" + conversationID
	audienceRef := "audience-cross-db"
	audienceHash := "audience-cross-db-hash"
	decisionRef := "decision-cross-db-" + conversationID
	body := agentTicketCreateBody{
		RunID: "run-cross-db-" + conversationID, ConversationID: conversationID,
		IdempotencyKey: "ticket-cross-db-" + conversationID, WorkType: "customer_case", Priority: "normal",
	}
	if _, err := controlPool.Exec(ctx, `
INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id, application_lifecycle_revision, registration_state)
VALUES ($1, $2, 'personal', $3, 1, 'active')`, spaceRef, orgID, subjectID); err != nil {
		t.Fatalf("seed Control Space: %v", err)
	}
	if _, err := controlPool.Exec(ctx, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, spaceRef); err != nil {
		t.Fatalf("seed Control authority revision: %v", err)
	}
	t.Cleanup(func() {
		_, _ = controlPool.Exec(ctx, `DELETE FROM space_owner_effect_reservations WHERE space_ref=$1`, spaceRef)
		_, _ = controlPool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref=$1`, spaceRef)
		_, _ = controlPool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref=$1`, spaceRef)
	})

	db := testLiveConversationCorePool(t, orgID, inboxID, conversationID)
	service := conversation.NewService(conversation.NewRepository(db.Pool), nil)
	requestSHA := "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	grantReceipt, err := service.CreateAgentTicketActionGrant(ctx, conversation.CreateAgentTicketActionGrantInput{
		OrgID: orgID, ConversationID: conversationID, ActionID: "tickets.create", SpaceRef: spaceRef,
		SubjectID: subjectID, RecipientAudienceRef: audienceRef, RecipientAudienceHash: audienceHash,
		RecipientAudienceRevision: 1, PrivacyPolicyRef: "privacy-cross-db", AuthorityRevision: 1,
		CreatedByUserID: subjectID, IdempotencyKey: "grant-create-" + conversationID,
		RequestSHA256: requestSHA, ControlDecisionRef: decisionRef,
	})
	if err != nil || grantReceipt == nil || grantReceipt.Grant == nil {
		t.Fatalf("create cross-database owner grant = %#v, err=%v", grantReceipt, err)
	}

	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	decisionVerifier, err := NewRunActionDecisionVerifier("control-cross-db-key", base64.RawStdEncoding.EncodeToString(publicKey))
	if err != nil {
		t.Fatalf("NewRunActionDecisionVerifier() error = %v", err)
	}
	decision := agentTicketTestDecision(body)
	decision.DecisionRef = decisionRef
	decision.ThreadID = "thread-cross-db-" + conversationID
	decision.OrgID = orgID
	decision.SpaceRef = spaceRef
	decision.SubjectID = subjectID
	decision.RecipientAudienceRef = audienceRef
	decision.RecipientAudienceHash = audienceHash
	decision.PrivacyPolicyRef = "privacy-cross-db"
	decision.AuthorityRevision = 1
	decision.IdempotencyKey = body.IdempotencyKey
	decision.PayloadDigest = agentTicketPayloadDigest(decision.RunID, decision.OrgID, body)
	body.ControlDecisionToken = signRunActionDecisionForTest(t, privateKey, "control-cross-db-key", decision)
	wireBody, err := json.Marshal(body)
	if err != nil {
		t.Fatalf("Marshal() error = %v", err)
	}

	coordinator := &liveControlOwnerEffectReservationCoordinator{
		pool: controlPool, orgID: orgID, spaceRef: spaceRef, subjectID: subjectID,
		runID: decision.RunID, threadID: decision.ThreadID, audienceRef: audienceRef,
		audienceHash: audienceHash, privacyRef: decision.PrivacyPolicyRef, release: make(chan struct{}), entered: make(chan struct{}),
	}
	handler := NewHandler(nil, service)
	handler.SetRunActionDecisionVerifier(decisionVerifier)
	handler.SetOwnerEffectReservationCoordinator(coordinator)
	// Keep the test router's default validator out of this cross-database proof:
	// the first authority read must observe revision 1, while the final read
	// after the Control revision bump must deny before Conversation writes.
	handler.SetRunActionAuthorityValidator(runActionAuthorityValidatorFunc(func(ctx context.Context, current runActionDecision) error {
		var authorityRevision int64
		if err := controlPool.QueryRow(ctx, `SELECT authority_revision FROM space_authority_revisions WHERE space_ref=$1`, current.SpaceRef).Scan(&authorityRevision); err != nil {
			return err
		}
		if authorityRevision != current.AuthorityRevision {
			return ErrRunActionAuthorityDenied
		}
		return nil
	}))
	router := agentTicketTestRouter(t, handler)
	request := httptest.NewRequest(http.MethodPost, "/internal/v1/agent-ticket-operations", bytes.NewReader(wireBody))
	signConversationRequest(t, request, wireBody, "execution-core", agentTicketExecutionSecret, "", "", "")
	responseCh := make(chan *httptest.ResponseRecorder, 1)
	go func() { responseCh <- performRequest(router, request) }()

	select {
	case <-coordinator.entered:
	case <-time.After(5 * time.Second):
		t.Fatal("Control reservation commit was not entered")
	}
	var controlStatus string
	if err := controlPool.QueryRow(ctx, `SELECT status FROM space_owner_effect_reservations WHERE operation_id=$1`, conversation.TicketOperationID(orgID, body.IdempotencyKey)).Scan(&controlStatus); err != nil {
		t.Fatalf("read committed Control reservation: %v", err)
	}
	if controlStatus != "committed" {
		t.Fatalf("Control reservation status = %q, want committed before owner release", controlStatus)
	}
	if _, err := controlPool.Exec(ctx, `UPDATE space_authority_revisions SET authority_revision=authority_revision+1 WHERE space_ref=$1`, spaceRef); err != nil {
		t.Fatalf("advance Control authority revision: %v", err)
	}
	if _, err := service.RevokeAgentTicketActionGrant(ctx, conversation.RevokeAgentTicketActionGrantInput{
		OrgID: orgID, ConversationID: conversationID, GrantID: grantReceipt.Grant.ID,
		SpaceRef: spaceRef, SubjectID: subjectID, RevokedByUserID: subjectID,
		IdempotencyKey: "grant-revoke-" + conversationID, RequestSHA256: requestSHA, ControlDecisionRef: decisionRef,
	}); err != nil {
		t.Fatalf("revoke Conversation owner grant: %v", err)
	}
	close(coordinator.release)

	var response *httptest.ResponseRecorder
	select {
	case response = <-responseCh:
	case <-time.After(5 * time.Second):
		t.Fatal("cross-database owner operation did not finish")
	}
	if response.Code != http.StatusForbidden || !bytes.Contains(response.Body.Bytes(), []byte(`"code":"forbidden"`)) {
		t.Fatalf("status/body = %d/%s, want final owner fence denial", response.Code, response.Body.String())
	}
	var operationStatus string
	if err := db.Pool.QueryRow(ctx, `SELECT status FROM conversation_ticket_operations WHERE org_id=$1 AND operation_id=$2`, orgID, conversation.TicketOperationID(orgID, body.IdempotencyKey)).Scan(&operationStatus); err != nil {
		t.Fatalf("read cross-database operation: %v", err)
	}
	if operationStatus != "cancelled" {
		t.Fatalf("cross-database operation status = %q, want cancelled", operationStatus)
	}
	var controlStatusAfter string
	if err := controlPool.QueryRow(ctx, `SELECT status FROM space_owner_effect_reservations WHERE operation_id=$1`, conversation.TicketOperationID(orgID, body.IdempotencyKey)).Scan(&controlStatusAfter); err != nil {
		t.Fatalf("read final Control reservation: %v", err)
	}
	if controlStatusAfter != "committed" {
		t.Fatalf("Control committed reservation changed after later revocation: %q", controlStatusAfter)
	}
	var ticketCount int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM conversation_tickets WHERE org_id=$1 AND conversation_id=$2`, orgID, conversationID).Scan(&ticketCount); err != nil {
		t.Fatalf("read cross-database tickets: %v", err)
	}
	if ticketCount != 0 {
		t.Fatalf("cross-database revoked owner wrote %d tickets", ticketCount)
	}
}

type liveControlOwnerEffectReservationCoordinator struct {
	pool         *pgxpool.Pool
	orgID        string
	spaceRef     string
	subjectID    string
	runID        string
	threadID     string
	audienceRef  string
	audienceHash string
	privacyRef   string
	release      chan struct{}
	entered      chan struct{}
	once         sync.Once
}

func (c *liveControlOwnerEffectReservationCoordinator) Ready() bool {
	return c != nil && c.pool != nil && c.release != nil && c.entered != nil
}

func (c *liveControlOwnerEffectReservationCoordinator) Reserve(ctx context.Context, _ string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
	reservationID := "reservation-" + commitment.OperationID
	_, err := c.pool.Exec(ctx, `
INSERT INTO space_owner_effect_reservations (
 operation_id, reservation_id, org_id, space_ref, subject_id, run_id, thread_id,
 action_id, action_schema_hash, payload_digest, idempotency_key, decision_ref, grant_ref,
 recipient_audience_ref, recipient_audience_hash, recipient_audience_revision,
 privacy_policy_ref, authority_revision, status, expires_at
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,1,$16,1,'reserved',NOW()+INTERVAL '5 minutes')
ON CONFLICT (operation_id) DO NOTHING`, commitment.OperationID, reservationID, c.orgID, c.spaceRef, c.subjectID,
		c.runID, c.threadID, commitment.ActionID, commitment.ActionSchemaHash, commitment.PayloadDigest,
		commitment.IdempotencyKey, commitment.DecisionRef, commitment.GrantRef, c.audienceRef, c.audienceHash, c.privacyRef)
	if err != nil {
		return ownerEffectReservationReceipt{}, err
	}
	return ownerEffectReservationReceipt{ReservationID: reservationID, OperationID: commitment.OperationID, Status: "reserved"}, nil
}

func (c *liveControlOwnerEffectReservationCoordinator) Commit(ctx context.Context, reservationID, _ string, commitment ownerEffectReservationCommitment) (ownerEffectReservationReceipt, error) {
	var updated int
	if err := c.pool.QueryRow(ctx, `
UPDATE space_owner_effect_reservations
SET status='committed', committed_at=NOW(), updated_at=NOW()
WHERE reservation_id=$1 AND operation_id=$2 AND status='reserved'
RETURNING 1`, reservationID, commitment.OperationID).Scan(&updated); err != nil {
		return ownerEffectReservationReceipt{}, err
	}
	if updated != 1 {
		return ownerEffectReservationReceipt{}, errors.New("Control reservation was not committed")
	}
	c.once.Do(func() { close(c.entered) })
	select {
	case <-c.release:
		return ownerEffectReservationReceipt{ReservationID: reservationID, OperationID: commitment.OperationID, Status: "committed"}, nil
	case <-ctx.Done():
		return ownerEffectReservationReceipt{}, ctx.Err()
	}
}
