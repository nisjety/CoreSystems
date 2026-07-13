package conversation

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/attestation"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PgxPool is the subset of *pgxpool.Pool that PGRepository depends on. Modelling
// it as an interface lets tests inject a fake pool/transaction so transactional
// behaviour (e.g. the AI-action review rollback) can be verified without a live
// database. *pgxpool.Pool satisfies this interface, so production wiring is
// unchanged.
type PgxPool interface {
	Begin(ctx context.Context) (pgx.Tx, error)
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
	Exec(ctx context.Context, sql string, arguments ...any) (pgconn.CommandTag, error)
}

type PGRepository struct {
	pool PgxPool
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
	// Keep a nil pool as a nil interface (not a non-nil interface wrapping a nil
	// pointer) so ensureConfigured's `r.pool == nil` guard still catches it.
	if pool == nil {
		return &PGRepository{}
	}
	return &PGRepository{pool: pool}
}

func (r *PGRepository) ListInboxes(ctx context.Context, orgID string) ([]Inbox, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if err := r.ensureDefaultInbox(ctx, orgID, "email"); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, name, channel, created_at, updated_at
FROM conversation_inboxes
WHERE org_id = $1
ORDER BY name ASC`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	inboxes := []Inbox{}
	for rows.Next() {
		var inbox Inbox
		if err := rows.Scan(&inbox.ID, &inbox.OrgID, &inbox.Name, &inbox.Channel, &inbox.CreatedAt, &inbox.UpdatedAt); err != nil {
			return nil, err
		}
		inboxes = append(inboxes, inbox)
	}
	return inboxes, rows.Err()
}

func (r *PGRepository) ListConversations(ctx context.Context, filter ListFilter) ([]ConversationSummary, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	rows, err := r.pool.Query(ctx, `
SELECT
	c.id, c.org_id, c.inbox_id, c.title, c.status, c.priority, c.channel,
	c.provider, c.provider_thread_id, c.assignee_user_id, c.assignee_name,
	c.last_message_preview, c.last_message_at, c.created_at, c.updated_at,
	COALESCE(ct.id, ''), COALESCE(ct.name, ''), COALESCE(ct.email, ''), COALESCE(ct.phone, '')
FROM conversations c
LEFT JOIN conversation_contacts ct ON ct.id = c.contact_id
WHERE c.org_id = $1
  AND ($2 = '' OR c.inbox_id = $2)
  AND ($3 = '' OR c.status = $3)
  AND ($4 = '' OR c.channel = $4)
  AND ($5 = '' OR ($5 = 'unassigned' AND c.assignee_user_id = '') OR ($5 <> 'unassigned' AND c.assignee_user_id = $5))
  AND ($6 = '' OR lower(c.title) LIKE '%' || lower($6) || '%' OR lower(c.last_message_preview) LIKE '%' || lower($6) || '%' OR lower(COALESCE(ct.email, '')) LIKE '%' || lower($6) || '%')
  AND ($7::timestamptz IS NULL OR (c.updated_at, c.id) < ($7::timestamptz, $8))
ORDER BY c.updated_at DESC, c.id DESC
LIMIT $9`, filter.OrgID, filter.InboxID, filter.Status, filter.Channel, filter.Assigned, filter.Query, filter.CursorUpdated, filter.CursorID, filter.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	conversations := []ConversationSummary{}
	for rows.Next() {
		conversation, err := scanConversationSummary(rows)
		if err != nil {
			return nil, err
		}
		conversations = append(conversations, conversation)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	return r.loadTagsForSummaries(ctx, conversations)
}

func (r *PGRepository) GetConversation(ctx context.Context, orgID, conversationID string) (*ConversationDetail, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	summary, err := r.getSummary(ctx, orgID, conversationID)
	if err != nil {
		return nil, err
	}
	messages, err := r.listMessages(ctx, orgID, conversationID)
	if err != nil {
		return nil, err
	}
	return &ConversationDetail{ConversationSummary: *summary, Messages: messages}, nil
}

// channelForProvider maps a connection provider key to the logical inbox/
// conversation channel. email/microsoft/google all map to "email" —
// preserving the exact pre-existing behavior for those providers, which is
// the only channel this repository originally supported. Anything else
// (whatsapp, messenger, …) gets its own channel so contacts/conversations
// never mix across providers; an unrecognized provider falls back to itself
// rather than silently joining the email inbox.
func channelForProvider(provider string) string {
	switch strings.ToLower(strings.TrimSpace(provider)) {
	case "", "email", "microsoft", "google":
		return "email"
	default:
		return strings.ToLower(strings.TrimSpace(provider))
	}
}

// channelLabel is the human-readable inbox name for a channel key.
func channelLabel(channel string) string {
	switch channel {
	case "email":
		return "Email"
	case "whatsapp":
		return "WhatsApp"
	case "messenger":
		return "Messenger"
	default:
		if channel == "" {
			return "Inbox"
		}
		return strings.ToUpper(channel[:1]) + channel[1:]
	}
}

// contactIdentityKey picks the identifier that uniquely names this event's
// sender within the org: email when present (the original design), else
// phone (WhatsApp), else a provider+event-scoped reference as a last resort
// so two different senders on a channel with neither never collide onto one
// contact record. The returned kind is namespaced into the hash input so an
// email and phone that happen to share literal text can't collide either.
func contactIdentityKey(event InboundEvent) (key, kind string) {
	if email := strings.TrimSpace(event.From.Email); email != "" {
		return strings.ToLower(email), "email"
	}
	if phone := strings.TrimSpace(event.From.Phone); phone != "" {
		return phone, "phone"
	}
	ref := firstNonEmptyString(event.ProviderThreadID, event.ProviderEventID, event.From.Name)
	return event.Provider + ":" + ref, "ref"
}

func firstNonEmptyString(values ...string) string {
	for _, v := range values {
		if strings.TrimSpace(v) != "" {
			return v
		}
	}
	return ""
}

func (r *PGRepository) StoreInboundEvent(ctx context.Context, event InboundEvent) (*StoredEventResult, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	existing, err := findIdempotentResult(ctx, tx, event.OrgID, event.IDempotencyKey)
	if err == nil {
		if err := tx.Commit(ctx); err != nil {
			return nil, err
		}
		committed = true
		return existing, nil
	}
	if !errors.Is(err, ErrNotFound) {
		return nil, err
	}

	channel := channelForProvider(event.Provider)
	inboxID := stableInboxID(event.OrgID, channel)
	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_inboxes (id, org_id, name, channel)
VALUES ($1, $2, $3, $4)
ON CONFLICT (org_id, channel) DO UPDATE SET updated_at = NOW()`, inboxID, event.OrgID, channelLabel(channel), channel); err != nil {
		return nil, err
	}

	// Contact identity key: email when present (matches the original,
	// email-only design), else phone (WhatsApp), else a provider-scoped
	// reference (Messenger PSID, or any channel with neither) so distinct
	// customers on non-email channels never collide on one contact record.
	// stableContactID's output IS the row's primary key, so ON CONFLICT (id)
	// upserts correctly regardless of which identity key produced it.
	contactKey, contactKeyKind := contactIdentityKey(event)
	contactID := stableContactID(event.OrgID, contactKeyKind+":"+contactKey)
	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_contacts (id, org_id, name, email, phone, updated_at)
VALUES ($1, $2, $3, $4, $5, NOW())
ON CONFLICT (id) DO UPDATE SET
	name = COALESCE(NULLIF(EXCLUDED.name, ''), conversation_contacts.name),
	email = COALESCE(NULLIF(EXCLUDED.email, ''), conversation_contacts.email),
	phone = COALESCE(NULLIF(EXCLUDED.phone, ''), conversation_contacts.phone),
	updated_at = NOW()`,
		contactID, event.OrgID, event.From.Name, event.From.Email, event.From.Phone); err != nil {
		return nil, err
	}

	conversationID, err := findConversationByThread(ctx, tx, event)
	if err != nil && !errors.Is(err, ErrNotFound) {
		return nil, err
	}
	createdConversation := false
	if conversationID == "" {
		conversationID = newID("conv")
		title := event.Subject
		if title == "" {
			title = "(no subject)"
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO conversations (
	id, org_id, inbox_id, contact_id, title, status, priority, channel, provider,
	provider_thread_id, last_message_preview, last_message_at, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, 'open', 'normal', $6, $7, $8, $9, $10, $10, $10)`,
			conversationID, event.OrgID, inboxID, contactID, title, channel, event.Provider, event.ProviderThreadID, preview(event.BodyText, event.BodyHTML), event.OccurredAt); err != nil {
			return nil, err
		}
		createdConversation = true
	}

	if event.ProviderThreadID != "" {
		if _, err := tx.Exec(ctx, `
INSERT INTO conversation_channel_thread_refs (id, org_id, conversation_id, provider, connection_id, provider_thread_id)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (org_id, provider, connection_id, provider_thread_id) DO NOTHING`,
			newID("threadref"), event.OrgID, conversationID, event.Provider, event.ConnectionID, event.ProviderThreadID); err != nil {
			return nil, err
		}
	}

	messageID := newID("msg")
	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_messages (
	id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
	body_text, body_html, internal, provider, provider_message_id, provider_event_id,
	occurred_at, created_at
) VALUES ($1, $2, $3, $4, 'customer', $5, $6, $7, $8, FALSE, $9, $10, $11, $12, $12)`,
		messageID, event.OrgID, conversationID, event.Direction, event.From.Name, event.From.Email,
		event.BodyText, event.BodyHTML, event.Provider, event.ProviderMessageID, event.ProviderEventID, event.OccurredAt); err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			existing, lookupErr := findByProviderMessage(ctx, tx, event)
			if lookupErr != nil {
				return nil, lookupErr
			}
			if err := tx.Commit(ctx); err != nil {
				return nil, err
			}
			committed = true
			return existing, nil
		}
		return nil, err
	}

	if _, err := tx.Exec(ctx, `
UPDATE conversations
SET last_message_preview = $3, last_message_at = $4, updated_at = $4
WHERE org_id = $1 AND id = $2`, event.OrgID, conversationID, preview(event.BodyText, event.BodyHTML), event.OccurredAt); err != nil {
		return nil, err
	}

	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_idempotency_keys (org_id, idempotency_key, outcome_conversation_id, outcome_message_id)
VALUES ($1, $2, $3, $4)
ON CONFLICT (org_id, idempotency_key) DO NOTHING`, event.OrgID, event.IDempotencyKey, conversationID, messageID); err != nil {
		return nil, err
	}

	eventType := "message.received"
	if createdConversation {
		eventType = "conversation.created"
	}
	if err := insertAuditAndEvent(ctx, tx, event.OrgID, conversationID, "", eventType, map[string]any{
		"message_id":        messageID,
		"provider":          event.Provider,
		"provider_event_id": event.ProviderEventID,
	}); err != nil {
		return nil, err
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true

	detail, err := r.GetConversation(ctx, event.OrgID, conversationID)
	if err != nil {
		return nil, err
	}
	message := findMessage(detail.Messages, messageID)
	return &StoredEventResult{Detail: detail, Message: message, Created: true}, nil
}

func (r *PGRepository) AddMessage(ctx context.Context, input AddMessageInput) (*Message, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	messageID := newID("msg")
	senderType := "agent"
	if input.Internal {
		senderType = "agent"
	}
	row := r.pool.QueryRow(ctx, `
WITH inserted AS (
	INSERT INTO conversation_messages (
		id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
		body_text, body_html, internal, provider, provider_message_id, occurred_at, created_at
	) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $13)
	RETURNING id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
		body_text, body_html, internal, provider, provider_message_id, provider_event_id, occurred_at, created_at
), updated AS (
	UPDATE conversations
	SET last_message_preview = $8, last_message_at = $13, updated_at = $13
	WHERE org_id = $2 AND id = $3
)
SELECT * FROM inserted`,
		messageID, input.OrgID, input.ConversationID, input.Direction, senderType, input.ActorName,
		input.ActorEmail, input.BodyText, input.BodyHTML, input.Internal, input.Provider, input.ProviderMessageID, input.OccurredAt)
	message, err := scanMessage(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	action := "message.sent"
	if input.Internal {
		action = "note.created"
	}
	if _, err := r.pool.Exec(ctx, `
INSERT INTO conversation_audit_events (id, org_id, conversation_id, actor_user_id, action, payload)
VALUES ($1, $2, $3, $4, $5, $6::jsonb)`,
		newID("audit"), input.OrgID, input.ConversationID, input.ActorUserID, action, mustJSON(map[string]any{"message_id": message.ID})); err != nil {
		return nil, err
	}
	return message, nil
}

func scanOutboundIntent(row scanner) (OutboundIntent, error) {
	var intent OutboundIntent
	err := row.Scan(
		&intent.ID,
		&intent.OrgID,
		&intent.IdempotencyKey,
		&intent.ConversationID,
		&intent.AIActionID,
		&intent.RequestFingerprint,
		&intent.Status,
		&intent.Provider,
		&intent.ConnectionID,
		&intent.ProviderThreadID,
		&intent.AuthorizationKind,
		&intent.ActorUserID,
		&intent.ApprovalID,
		&intent.ActionID,
		&intent.Operation,
		&intent.PayloadSHA256,
		&intent.ProviderMessageID,
		&intent.MessageID,
		&intent.ErrorCode,
		&intent.CreatedAt,
		&intent.UpdatedAt,
	)
	return intent, err
}

const outboundIntentColumns = `
id, org_id, idempotency_key, conversation_id, ai_action_id,
request_fingerprint, status, provider, connection_id, provider_thread_id,
authorization_kind, actor_user_id, approval_id, action_id, operation, payload_sha256,
provider_message_id, message_id, error_code, created_at, updated_at`

// ClaimOutboundIntent is the durable compare-and-set before any provider call.
// The first claimant owns the attempt. Every replay observes the stored state;
// a reused key with a different fingerprint fails closed as ErrConflict.
func (r *PGRepository) ClaimOutboundIntent(ctx context.Context, input OutboundIntentClaimInput) (*OutboundIntentClaim, error) {
	if err := validateOutboundIntentClaim(input); err != nil {
		return nil, err
	}
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, `
	INSERT INTO conversation_outbound_intents (
	id, org_id, idempotency_key, conversation_id, ai_action_id,
	request_fingerprint, status, provider, connection_id, provider_thread_id,
	authorization_kind, actor_user_id, approval_id, action_id, operation, payload_sha256
) VALUES ($1, $2, $3, $4, $5, $6, 'sending', $7, $8, $9, $10, $11, $12, $13, $14, $15)
ON CONFLICT (org_id, idempotency_key) DO NOTHING
RETURNING `+outboundIntentColumns,
		input.IntentID, input.OrgID, input.IdempotencyKey, input.ConversationID,
		input.AIActionID, input.RequestFingerprint, input.Provider, input.ConnectionID, input.ProviderThreadID,
		input.AuthorizationKind, input.ActorUserID, input.ApprovalID, input.ActionID, input.Operation, input.PayloadSHA256)
	intent, err := scanOutboundIntent(row)
	if err == nil {
		return &OutboundIntentClaim{Intent: intent, Claimed: true}, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return nil, err
	}

	intent, err = scanOutboundIntent(r.pool.QueryRow(ctx, `
SELECT `+outboundIntentColumns+`
FROM conversation_outbound_intents
WHERE org_id = $1 AND idempotency_key = $2`, input.OrgID, input.IdempotencyKey))
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrConflict
		}
		return nil, err
	}
	if !outboundIntentBindingMatches(intent, input) {
		return nil, fmt.Errorf("%w: idempotency key belongs to a different outbound request", ErrConflict)
	}
	if intent.Status == OutboundIntentRetryable {
		reclaimed, reclaimErr := scanOutboundIntent(r.pool.QueryRow(ctx, `
UPDATE conversation_outbound_intents
SET status = 'sending', error_code = '', updated_at = NOW()
WHERE org_id = $1 AND idempotency_key = $2 AND status = 'retryable'
RETURNING `+outboundIntentColumns, input.OrgID, input.IdempotencyKey))
		if reclaimErr == nil {
			return &OutboundIntentClaim{Intent: reclaimed, Claimed: true}, nil
		}
		if !errors.Is(reclaimErr, pgx.ErrNoRows) {
			return nil, reclaimErr
		}
	}
	return &OutboundIntentClaim{Intent: intent, Claimed: false}, nil
}

func validateOutboundIntentClaim(input OutboundIntentClaimInput) error {
	for name, value := range map[string]string{
		"intent_id": input.IntentID, "org_id": input.OrgID,
		"idempotency_key": input.IdempotencyKey, "conversation_id": input.ConversationID,
		"request_fingerprint": input.RequestFingerprint, "provider": input.Provider,
		"connection_id": input.ConnectionID, "authorization_kind": input.AuthorizationKind,
		"actor_user_id": input.ActorUserID, "action_id": input.ActionID,
		"operation": input.Operation, "payload_sha256": input.PayloadSHA256,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("%w: outbound intent %s is required", ErrInvalidInput, name)
		}
	}
	if !attestation.IsLowerHexSHA256(input.PayloadSHA256) {
		return fmt.Errorf("%w: outbound intent payload_sha256 is invalid", ErrInvalidInput)
	}
	switch input.AuthorizationKind {
	case attestation.AuthorizationHumanIntent:
		if input.ApprovalID != "" || input.AIActionID != "" || input.ActionID != input.IntentID {
			return fmt.Errorf("%w: human intent authorization binding is invalid", ErrInvalidInput)
		}
	case attestation.AuthorizationHumanApprovedAIAction:
		if input.ApprovalID == "" || input.ApprovalID != input.ActionID || input.AIActionID != input.ActionID {
			return fmt.Errorf("%w: approved AI authorization binding is invalid", ErrInvalidInput)
		}
	default:
		return fmt.Errorf("%w: outbound intent authorization_kind is invalid", ErrInvalidInput)
	}
	return nil
}

func outboundIntentBindingMatches(intent OutboundIntent, input OutboundIntentClaimInput) bool {
	return intent.ID == input.IntentID &&
		intent.RequestFingerprint == input.RequestFingerprint &&
		intent.ConversationID == input.ConversationID &&
		intent.AIActionID == input.AIActionID &&
		intent.Provider == input.Provider && intent.ConnectionID == input.ConnectionID &&
		intent.ProviderThreadID == input.ProviderThreadID &&
		intent.AuthorizationKind == input.AuthorizationKind &&
		intent.ActorUserID == input.ActorUserID && intent.ApprovalID == input.ApprovalID &&
		intent.ActionID == input.ActionID && intent.Operation == input.Operation &&
		intent.PayloadSHA256 == input.PayloadSHA256
}

// FinalizeOutboundIntent atomically persists the submitted message, audit
// record, optional AI-action execution state, and ledger outcome.
func (r *PGRepository) FinalizeOutboundIntent(ctx context.Context, input OutboundIntentFinalizeInput) (*Message, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	intent, err := scanOutboundIntent(tx.QueryRow(ctx, `
SELECT `+outboundIntentColumns+`
FROM conversation_outbound_intents
WHERE org_id = $1 AND idempotency_key = $2
FOR UPDATE`, input.OrgID, input.IdempotencyKey))
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	if intent.RequestFingerprint != input.RequestFingerprint || intent.AIActionID != input.AIActionID {
		return nil, fmt.Errorf("%w: outbound finalization does not match claimed request", ErrConflict)
	}
	if intent.Status != OutboundIntentSending {
		return nil, fmt.Errorf("%w: outbound intent is already %s", ErrConflict, intent.Status)
	}

	messageInput := input.Message
	if messageInput.OrgID != intent.OrgID || messageInput.ConversationID != intent.ConversationID || messageInput.Internal || messageInput.Direction != DirectionOutbound {
		return nil, fmt.Errorf("%w: outbound message does not match claimed tenant and conversation", ErrConflict)
	}
	messageInput.Provider = intent.Provider
	messageInput.ProviderMessageID = strings.TrimSpace(input.ProviderMessageID)
	messageID := newID("msg")
	message, err := scanMessage(tx.QueryRow(ctx, `
INSERT INTO conversation_messages (
	id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
	body_text, body_html, internal, provider, provider_message_id, occurred_at, created_at
) VALUES ($1, $2, $3, $4, 'agent', $5, $6, $7, $8, FALSE, $9, $10, $11, $11)
RETURNING id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
	body_text, body_html, internal, provider, provider_message_id, provider_event_id, occurred_at, created_at`,
		messageID, messageInput.OrgID, messageInput.ConversationID, messageInput.Direction,
		messageInput.ActorName, messageInput.ActorEmail, messageInput.BodyText, messageInput.BodyHTML,
		messageInput.Provider, messageInput.ProviderMessageID, messageInput.OccurredAt))
	if err != nil {
		return nil, err
	}

	tag, err := tx.Exec(ctx, `
UPDATE conversations
SET last_message_preview = $3, last_message_at = $4, updated_at = $4
WHERE org_id = $1 AND id = $2`, messageInput.OrgID, messageInput.ConversationID, preview(messageInput.BodyText, messageInput.BodyHTML), messageInput.OccurredAt)
	if err != nil {
		return nil, err
	}
	if tag.RowsAffected() != 1 {
		return nil, ErrNotFound
	}

	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_audit_events (id, org_id, conversation_id, actor_user_id, action, payload)
VALUES ($1, $2, $3, $4, 'message.submitted', $5::jsonb)`,
		newID("audit"), messageInput.OrgID, messageInput.ConversationID, messageInput.ActorUserID,
		mustJSON(map[string]any{"message_id": message.ID, "idempotency_key": input.IdempotencyKey})); err != nil {
		return nil, err
	}

	if input.AIActionID != "" {
		tag, err = tx.Exec(ctx, `
UPDATE conversation_ai_actions
SET status = 'executed', updated_at = NOW()
WHERE org_id = $1 AND id = $2 AND status = 'approved'`, input.OrgID, input.AIActionID)
		if err != nil {
			return nil, err
		}
		if tag.RowsAffected() != 1 {
			return nil, fmt.Errorf("%w: approved AI action cannot be finalized", ErrConflict)
		}
	}

	tag, err = tx.Exec(ctx, `
UPDATE conversation_outbound_intents
SET status = 'submitted', provider_message_id = $3, message_id = $4,
	error_code = '', updated_at = NOW()
WHERE org_id = $1 AND idempotency_key = $2 AND status = 'sending'`,
		input.OrgID, input.IdempotencyKey, message.ProviderMessageID, message.ID)
	if err != nil {
		return nil, err
	}
	if tag.RowsAffected() != 1 {
		return nil, fmt.Errorf("%w: outbound intent was not finalizable", ErrConflict)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true
	return message, nil
}

// MarkOutboundIntentOutcome records only bounded error metadata. Both unknown
// and failed are terminal for automatic retry; reconciliation or a new human
// intent is required.
func (r *PGRepository) MarkOutboundIntentOutcome(ctx context.Context, input OutboundIntentOutcomeInput) error {
	if input.Status != OutboundIntentFailed && input.Status != OutboundIntentUnknown && input.Status != OutboundIntentRetryable {
		return fmt.Errorf("%w: invalid outbound intent outcome", ErrInvalidInput)
	}
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()
	tag, err := tx.Exec(ctx, `
UPDATE conversation_outbound_intents
SET status = $3, error_code = $4, updated_at = NOW()
WHERE org_id = $1 AND idempotency_key = $2 AND status = 'sending'`,
		input.OrgID, input.IdempotencyKey, input.Status, boundedOutboundErrorCode(input.ErrorCode))
	if err != nil {
		return err
	}
	if tag.RowsAffected() == 0 {
		var current string
		if err := tx.QueryRow(ctx, `
SELECT status FROM conversation_outbound_intents
WHERE org_id = $1 AND idempotency_key = $2`, input.OrgID, input.IdempotencyKey).Scan(&current); err != nil {
			if errors.Is(err, pgx.ErrNoRows) {
				return ErrNotFound
			}
			return err
		}
		if current != input.Status {
			return fmt.Errorf("%w: outbound intent is already %s", ErrConflict, current)
		}
	}
	if input.AIActionID != "" && input.Status != OutboundIntentRetryable {
		if _, err := tx.Exec(ctx, `
UPDATE conversation_ai_actions
SET status = $3, updated_at = NOW()
WHERE org_id = $1 AND id = $2 AND status = 'approved'`, input.OrgID, input.AIActionID, input.Status); err != nil {
			return err
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return err
	}
	committed = true
	return nil
}

func boundedOutboundErrorCode(value string) string {
	value = strings.TrimSpace(value)
	if value == "" || len(value) > 64 {
		return "outbound_error"
	}
	for _, char := range value {
		if (char < 'a' || char > 'z') && (char < 'A' || char > 'Z') &&
			(char < '0' || char > '9') && char != '_' && char != '-' && char != '.' {
			return "outbound_error"
		}
	}
	return value
}

func (r *PGRepository) GetMessage(ctx context.Context, orgID, messageID string) (*Message, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	message, err := scanMessage(r.pool.QueryRow(ctx, `
SELECT id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
	body_text, body_html, internal, provider, provider_message_id, provider_event_id, occurred_at, created_at
FROM conversation_messages
WHERE org_id = $1 AND id = $2`, orgID, messageID))
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return message, nil
}

func (r *PGRepository) UpdateStatus(ctx context.Context, input StatusUpdate) (*ConversationDetail, error) {
	if _, err := r.pool.Exec(ctx, `
UPDATE conversations
SET status = $3, updated_at = NOW()
WHERE org_id = $1 AND id = $2`, input.OrgID, input.ConversationID, input.Status); err != nil {
		return nil, err
	}
	if err := r.insertSimpleAudit(ctx, input.OrgID, input.ConversationID, input.ActorUserID, "status.changed", map[string]any{"status": input.Status}); err != nil {
		return nil, err
	}
	return r.GetConversation(ctx, input.OrgID, input.ConversationID)
}

func (r *PGRepository) UpdateAssignment(ctx context.Context, input AssignmentUpdate) (*ConversationDetail, error) {
	if _, err := r.pool.Exec(ctx, `
UPDATE conversations
SET assignee_user_id = $3, assignee_name = $4, updated_at = NOW()
WHERE org_id = $1 AND id = $2`, input.OrgID, input.ConversationID, input.AssigneeUserID, input.AssigneeName); err != nil {
		return nil, err
	}
	if err := r.insertSimpleAudit(ctx, input.OrgID, input.ConversationID, input.ActorUserID, "assignment.changed", map[string]any{
		"assignee_user_id": input.AssigneeUserID,
		"assignee_name":    input.AssigneeName,
	}); err != nil {
		return nil, err
	}
	return r.GetConversation(ctx, input.OrgID, input.ConversationID)
}

func (r *PGRepository) AddTag(ctx context.Context, orgID, conversationID, tag string) (*ConversationDetail, error) {
	tagID := "tag_" + hexLower(orgID+":"+tag)
	if err := r.pool.QueryRow(ctx, `
INSERT INTO conversation_tags (id, org_id, name)
VALUES ($1, $2, $3)
ON CONFLICT (org_id, lower(name)) DO UPDATE SET name = EXCLUDED.name
RETURNING id`, tagID, orgID, tag).Scan(&tagID); err != nil {
		return nil, err
	}
	if _, err := r.pool.Exec(ctx, `
INSERT INTO conversation_tag_links (org_id, conversation_id, tag_id)
VALUES ($1, $2, $3)
ON CONFLICT DO NOTHING`, orgID, conversationID, tagID); err != nil {
		return nil, err
	}
	if err := r.insertSimpleAudit(ctx, orgID, conversationID, "", "tag.added", map[string]any{"tag": tag}); err != nil {
		return nil, err
	}
	return r.GetConversation(ctx, orgID, conversationID)
}

func (r *PGRepository) RemoveTag(ctx context.Context, orgID, conversationID, tag string) (*ConversationDetail, error) {
	if _, err := r.pool.Exec(ctx, `
DELETE FROM conversation_tag_links link
USING conversation_tags tag
WHERE link.tag_id = tag.id
  AND link.org_id = $1
  AND link.conversation_id = $2
  AND lower(tag.name) = lower($3)`, orgID, conversationID, tag); err != nil {
		return nil, err
	}
	if err := r.insertSimpleAudit(ctx, orgID, conversationID, "", "tag.removed", map[string]any{"tag": tag}); err != nil {
		return nil, err
	}
	return r.GetConversation(ctx, orgID, conversationID)
}

func (r *PGRepository) ReviewAIAction(ctx context.Context, input AIActionReview) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()
	tag, err := tx.Exec(ctx, `
UPDATE conversation_ai_actions
SET status = $3, reviewed_by = $4, reviewed_at = $5, updated_at = $5
WHERE org_id = $1 AND id = $2 AND status = 'suggested'`, input.OrgID, input.AIActionID, input.Decision, input.ReviewerID, input.OccurredAt)
	if err != nil {
		return err
	}
	// The compare-and-set above is the HITL boundary: only a suggested action may
	// receive its first decision. Distinguish a missing/foreign-org id from an
	// existing terminal action without weakening tenant scope.
	if tag.RowsAffected() == 0 {
		var currentStatus string
		err := tx.QueryRow(ctx, `
SELECT status
FROM conversation_ai_actions
WHERE org_id = $1 AND id = $2`, input.OrgID, input.AIActionID).Scan(&currentStatus)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return err
		}
		return fmt.Errorf("%w: AI action is already %s", ErrConflict, currentStatus)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_ai_reviews (id, org_id, ai_action_id, reviewer_user_id, decision, comment, created_at)
VALUES ($1, $2, $3, $4, $5, $6, $7)`, newID("airev"), input.OrgID, input.AIActionID, input.ReviewerID, input.Decision, input.Comment, input.OccurredAt); err != nil {
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return err
	}
	committed = true
	return nil
}

func (r *PGRepository) ListAIActions(ctx context.Context, filter AIActionListFilter) ([]AIAction, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, conversation_id, kind, status, payload, created_by, reviewed_by, reviewed_at, created_at, updated_at
FROM conversation_ai_actions
WHERE org_id = $1
  AND ($2 = '' OR status = $2)
  AND ($3 = '' OR conversation_id = $3)
ORDER BY created_at DESC, id DESC
LIMIT $4`, filter.OrgID, filter.Status, filter.ConversationID, filter.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	actions := []AIAction{}
	for rows.Next() {
		action, err := scanAIAction(rows)
		if err != nil {
			return nil, err
		}
		actions = append(actions, action)
	}
	return actions, rows.Err()
}

func scanAIAction(row scanner) (AIAction, error) {
	var action AIAction
	var payloadBytes []byte
	if err := row.Scan(
		&action.ID, &action.OrgID, &action.ConversationID, &action.Kind, &action.Status,
		&payloadBytes, &action.CreatedBy, &action.ReviewedBy, &action.ReviewedAt,
		&action.CreatedAt, &action.UpdatedAt,
	); err != nil {
		return AIAction{}, err
	}
	if len(payloadBytes) > 0 {
		_ = json.Unmarshal(payloadBytes, &action.Payload)
	}
	if action.Payload == nil {
		action.Payload = map[string]any{}
	}
	return action, nil
}

// GetAIAction returns a single model-proposed action scoped to the org. Returns
// ErrNotFound for a missing id or a foreign-org id, so it never leaks an action
// across tenants (mirrors the ReviewAIAction org-scoping).
func (r *PGRepository) GetAIAction(ctx context.Context, orgID, id string) (*AIAction, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, `
SELECT id, org_id, conversation_id, kind, status, payload, created_by, reviewed_by, reviewed_at, created_at, updated_at
FROM conversation_ai_actions
WHERE org_id = $1 AND id = $2`, orgID, id)
	action, err := scanAIAction(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	return &action, nil
}

// MarkAIActionExecuted atomically claims an approved action for execution by
// transitioning status approved → executed. This is the idempotency gate for
// the HITL executor: exactly one delivery of a duplicated or redelivered
// ai_action.reviewed event observes RowsAffected == 1 (and may apply side
// effects); every other delivery observes 0 and must skip. Org-scoped.
func (r *PGRepository) MarkAIActionExecuted(ctx context.Context, orgID, id string) (bool, error) {
	if err := r.ensureConfigured(); err != nil {
		return false, err
	}
	tag, err := r.pool.Exec(ctx, `
UPDATE conversation_ai_actions
SET status = 'executed', updated_at = NOW()
WHERE org_id = $1 AND id = $2 AND status = 'approved'`, orgID, id)
	if err != nil {
		return false, err
	}
	return tag.RowsAffected() == 1, nil
}

// UnmarkAIActionExecuted reverts a claim (executed → approved) so a transient
// failure while applying side effects leaves the action eligible for a
// JetStream redelivery rather than stranded as executed-but-unapplied.
func (r *PGRepository) UnmarkAIActionExecuted(ctx context.Context, orgID, id string) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	_, err := r.pool.Exec(ctx, `
UPDATE conversation_ai_actions
SET status = 'approved', updated_at = NOW()
WHERE org_id = $1 AND id = $2 AND status = 'executed'`, orgID, id)
	return err
}

// CreateAIAction inserts a model-proposed action of an arbitrary kind at the
// default 'suggested' status, org-scoped. It generalizes the hard-coded insert
// in RecordTicketClassification so a human/hook (or the model-proposed consumer)
// can queue any allowed action — currently draft.reply — into the review queue.
func (r *PGRepository) CreateAIAction(ctx context.Context, input CreateAIActionInput) (*AIAction, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, `
INSERT INTO conversation_ai_actions (
	id, org_id, conversation_id, kind, status, payload, created_by, created_at, updated_at
) VALUES ($1, $2, $3, $4, 'suggested', $5::jsonb, $6, NOW(), NOW())
RETURNING id, org_id, conversation_id, kind, status, payload, created_by, reviewed_by, reviewed_at, created_at, updated_at`,
		newID("aiact"), input.OrgID, input.ConversationID, input.Kind, mustJSON(input.Payload), createdBy(input.CreatedBy))
	action, err := scanAIAction(row)
	if err != nil {
		return nil, err
	}
	return &action, nil
}

// GetChannelThreadRefByConversation resolves the outbound send target for a
// conversation: the most recent provider/connection/thread the conversation is
// bound to. Org-scoped; returns ErrNotFound when no channel ref exists (e.g. a
// purely internal conversation), so the executor skips without claiming.
func (r *PGRepository) GetChannelThreadRefByConversation(ctx context.Context, orgID, conversationID string) (*ChannelThreadRef, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	var ref ChannelThreadRef
	if err := r.pool.QueryRow(ctx, `
SELECT org_id, conversation_id, provider, connection_id, provider_thread_id
FROM conversation_channel_thread_refs
WHERE org_id = $1 AND conversation_id = $2
ORDER BY created_at DESC
LIMIT 1`, orgID, conversationID).Scan(
		&ref.OrgID, &ref.ConversationID, &ref.Provider, &ref.ConnectionID, &ref.ProviderThreadID,
	); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	return &ref, nil
}

func (r *PGRepository) ListTickets(ctx context.Context, filter TicketListFilter) ([]Ticket, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	rows, err := r.pool.Query(ctx, `
SELECT
	t.id, t.org_id, t.conversation_id, t.ticket_key, t.status, t.priority, t.severity,
	t.category, t.intent, t.assignee_user_id, t.assignee_name, t.team_id, t.team_name,
	t.due_at, t.source, t.ai_confidence, t.ai_reason, t.created_by, t.created_at, t.updated_at,
	t.waiting_since, t.last_customer_reply_at, t.first_response_at, t.resolved_at, t.snoozed_until,
	t.sla_policy_id, t.escalation_at, t.labels,
	c.id, c.org_id, c.inbox_id, c.title, c.status, c.priority, c.channel,
	c.provider, c.provider_thread_id, c.assignee_user_id, c.assignee_name,
	c.last_message_preview, c.last_message_at, c.created_at, c.updated_at,
	COALESCE(ct.id, ''), COALESCE(ct.name, ''), COALESCE(ct.email, ''), COALESCE(ct.phone, '')
FROM conversation_tickets t
JOIN conversations c ON c.id = t.conversation_id AND c.org_id = t.org_id
LEFT JOIN conversation_contacts ct ON ct.id = c.contact_id
WHERE t.org_id = $1
  AND ($2 = '' OR t.status = $2)
  AND ($3 = '' OR ($3 = 'unassigned' AND t.assignee_user_id = '') OR ($3 <> 'unassigned' AND t.assignee_user_id = $3))
  AND ($5 = '' OR t.team_id = $5)
  AND ($6 = '' OR $6 = ANY(t.labels))
  AND ($7 = '' OR t.priority = $7)
  AND ($8 = '' OR t.severity = $8)
  AND (
    $9 = ''
    OR ($9 = 'ok' AND (t.due_at IS NULL OR t.due_at > NOW() + INTERVAL '24 hours') AND t.escalation_at IS NULL)
    OR ($9 = 'risk' AND t.due_at IS NOT NULL AND t.due_at <= NOW() + INTERVAL '24 hours' AND t.due_at >= NOW() AND t.status NOT IN ('resolved', 'closed'))
    OR ($9 = 'breached' AND ((t.due_at IS NOT NULL AND t.due_at < NOW()) OR (t.escalation_at IS NOT NULL AND t.escalation_at <= NOW())) AND t.status NOT IN ('resolved', 'closed'))
  )
  AND (
    $10 = ''
    OR lower(t.ticket_key) LIKE '%' || lower($10) || '%'
    OR lower(t.category) LIKE '%' || lower($10) || '%'
    OR lower(t.intent) LIKE '%' || lower($10) || '%'
    OR lower(c.title) LIKE '%' || lower($10) || '%'
    OR lower(c.last_message_preview) LIKE '%' || lower($10) || '%'
    OR lower(COALESCE(ct.email, '')) LIKE '%' || lower($10) || '%'
    OR lower(array_to_string(t.labels, ' ')) LIKE '%' || lower($10) || '%'
  )
  AND (
    $4 = ''
    OR ($4 = 'suggested' AND t.status = 'suggested')
    OR ($4 = 'my' AND $3 <> '' AND t.assignee_user_id = $3)
    OR ($4 = 'unassigned' AND t.assignee_user_id = '')
    OR ($4 = 'sla-risk' AND t.due_at IS NOT NULL AND t.due_at <= NOW() + INTERVAL '24 hours' AND t.status NOT IN ('resolved', 'closed'))
    OR ($4 = 'escalated' AND (t.status = 'escalated' OR t.severity IN ('high', 'critical')))
    OR ($4 = 'waiting-customer' AND t.status = 'waiting_customer')
    OR ($4 = 'waiting-team' AND t.status = 'waiting_team')
    OR ($4 = 'resolved' AND t.status IN ('resolved', 'closed'))
    OR ($4 = 'snoozed' AND t.status = 'snoozed')
  )
ORDER BY
  CASE WHEN t.status = 'suggested' THEN 0 ELSE 1 END,
  CASE WHEN t.due_at IS NULL THEN 1 ELSE 0 END,
  t.due_at ASC NULLS LAST,
  t.updated_at DESC,
  t.id DESC
LIMIT $11`, filter.OrgID, filter.Status, filter.Assigned, filter.Queue, filter.TeamID, filter.Label, filter.Priority, filter.Severity, filter.SLAState, filter.Query, filter.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	tickets := []Ticket{}
	for rows.Next() {
		ticket, err := scanTicketWithConversation(rows)
		if err != nil {
			return nil, err
		}
		tickets = append(tickets, ticket)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	tickets, err = r.loadLinksForTickets(ctx, filter.OrgID, tickets)
	if err != nil {
		return nil, err
	}
	return r.loadChecklistsForTickets(ctx, filter.OrgID, tickets)
}

func (r *PGRepository) GetTicket(ctx context.Context, orgID, ticketID string) (*Ticket, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, ticketSelectSQL()+` WHERE t.org_id = $1 AND t.id = $2`, orgID, ticketID)
	ticket, err := scanTicketWithConversation(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	tickets, err := r.loadLinksForTickets(ctx, orgID, []Ticket{ticket})
	if err != nil {
		return nil, err
	}
	tickets, err = r.loadChecklistsForTickets(ctx, orgID, tickets)
	if err != nil {
		return nil, err
	}
	ticket = tickets[0]
	return &ticket, nil
}

func (r *PGRepository) GetTicketByConversation(ctx context.Context, orgID, conversationID string) (*Ticket, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, ticketSelectSQL()+` WHERE t.org_id = $1 AND t.conversation_id = $2`, orgID, conversationID)
	ticket, err := scanTicketWithConversation(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	tickets, err := r.loadLinksForTickets(ctx, orgID, []Ticket{ticket})
	if err != nil {
		return nil, err
	}
	tickets, err = r.loadChecklistsForTickets(ctx, orgID, tickets)
	if err != nil {
		return nil, err
	}
	ticket = tickets[0]
	return &ticket, nil
}

func (r *PGRepository) CreateTicket(ctx context.Context, input CreateTicketInput) (*Ticket, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	ticketID := newID("ticket")
	ticketKey := stableTicketKey(input.OrgID, input.ConversationID)
	_, err := r.pool.Exec(ctx, `
INSERT INTO conversation_tickets (
	id, org_id, conversation_id, ticket_key, status, priority, severity, category, intent,
	assignee_user_id, assignee_name, team_id, team_name, due_at, source, ai_confidence,
	ai_reason, created_by, waiting_since, last_customer_reply_at, first_response_at, resolved_at,
	snoozed_until, sla_policy_id, escalation_at, labels, created_at, updated_at
) VALUES (
	$1, $2, $3, $4, $5, $6, $7, $8, $9,
	$10, $11, $12, $13, $14, $15, $16,
	$17, $18, $19, $20, $21, $22,
	$23, $24, $25, $26, NOW(), NOW()
)`,
		ticketID, input.OrgID, input.ConversationID, ticketKey, input.Status, input.Priority, input.Severity,
		input.Category, input.Intent, input.AssigneeUserID, input.AssigneeName, input.TeamID, input.TeamName,
		input.DueAt, input.Source, input.AIConfidence, input.AIReason, input.CreatedBy, input.WaitingSince,
		input.LastCustomerReplyAt, input.FirstResponseAt, input.ResolvedAt, input.SnoozedUntil,
		input.SLAPolicyID, input.EscalationAt, input.Labels)
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == "23505" {
			return nil, ErrConflict
		}
		return nil, err
	}
	if err := r.insertSimpleAudit(ctx, input.OrgID, input.ConversationID, input.ActorUserID, "ticket.created", map[string]any{
		"ticket_id":  ticketID,
		"ticket_key": ticketKey,
		"status":     input.Status,
	}); err != nil {
		return nil, err
	}
	return r.GetTicket(ctx, input.OrgID, ticketID)
}

func (r *PGRepository) UpdateTicket(ctx context.Context, input UpdateTicketInput) (*Ticket, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	assignments := []string{}
	args := []any{input.OrgID, input.TicketID}
	add := func(column string, value any) {
		args = append(args, value)
		assignments = append(assignments, fmt.Sprintf("%s = $%d", column, len(args)))
	}
	if input.Status != nil {
		add("status", *input.Status)
	}
	if input.Priority != nil {
		add("priority", *input.Priority)
	}
	if input.Severity != nil {
		add("severity", *input.Severity)
	}
	if input.Category != nil {
		add("category", *input.Category)
	}
	if input.Intent != nil {
		add("intent", *input.Intent)
	}
	if input.AssigneeUserID != nil {
		add("assignee_user_id", *input.AssigneeUserID)
	}
	if input.AssigneeName != nil {
		add("assignee_name", *input.AssigneeName)
	}
	if input.TeamID != nil {
		add("team_id", *input.TeamID)
	}
	if input.TeamName != nil {
		add("team_name", *input.TeamName)
	}
	if input.DueAt != nil {
		add("due_at", *input.DueAt)
	}
	if input.Source != nil {
		add("source", *input.Source)
	}
	if input.AIConfidence != nil {
		add("ai_confidence", *input.AIConfidence)
	}
	if input.AIReason != nil {
		add("ai_reason", *input.AIReason)
	}
	if input.WaitingSince != nil {
		add("waiting_since", *input.WaitingSince)
	}
	if input.LastCustomerReplyAt != nil {
		add("last_customer_reply_at", *input.LastCustomerReplyAt)
	}
	if input.FirstResponseAt != nil {
		add("first_response_at", *input.FirstResponseAt)
	}
	if input.ResolvedAt != nil {
		add("resolved_at", *input.ResolvedAt)
	}
	if input.SnoozedUntil != nil {
		add("snoozed_until", *input.SnoozedUntil)
	}
	if input.SLAPolicyID != nil {
		add("sla_policy_id", *input.SLAPolicyID)
	}
	if input.EscalationAt != nil {
		add("escalation_at", *input.EscalationAt)
	}
	if input.Labels != nil {
		add("labels", *input.Labels)
	}
	if len(assignments) == 0 {
		return r.GetTicket(ctx, input.OrgID, input.TicketID)
	}
	assignments = append(assignments, "updated_at = NOW()")
	tag, err := r.pool.Exec(ctx, `
UPDATE conversation_tickets
SET `+strings.Join(assignments, ", ")+`
WHERE org_id = $1 AND id = $2`, args...)
	if err != nil {
		return nil, err
	}
	if tag.RowsAffected() == 0 {
		return nil, ErrNotFound
	}
	ticket, err := r.GetTicket(ctx, input.OrgID, input.TicketID)
	if err != nil {
		return nil, err
	}
	if err := r.insertSimpleAudit(ctx, input.OrgID, ticket.ConversationID, input.ActorUserID, "ticket.updated", map[string]any{
		"ticket_id": input.TicketID,
	}); err != nil {
		return nil, err
	}
	return ticket, nil
}

func (r *PGRepository) LinkTicketResource(ctx context.Context, input LinkTicketResourceInput) (*TicketLinkedResource, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	ticket, err := r.GetTicket(ctx, input.OrgID, input.TicketID)
	if err != nil {
		return nil, err
	}
	linkID := newID("link")
	row := r.pool.QueryRow(ctx, `
INSERT INTO conversation_linked_resources (
	id, org_id, ticket_id, conversation_id, link_type, resource_kind, resource_id, resource_url,
	label, metadata, created_by_user_id, created_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, $11, NOW())
RETURNING id, org_id, ticket_id, conversation_id, link_type, resource_kind, resource_id, resource_url,
	label, metadata, created_by_user_id, created_at`,
		linkID, input.OrgID, input.TicketID, ticket.ConversationID, input.LinkType, input.ResourceKind, input.ResourceID,
		input.ResourceURL, input.Label, mustJSON(input.Metadata), input.CreatedByUserID)
	link, err := scanTicketLinkedResource(row)
	if err != nil {
		return nil, err
	}
	if err := r.insertSimpleAudit(ctx, input.OrgID, ticket.ConversationID, input.CreatedByUserID, "ticket.linked", map[string]any{
		"ticket_id":     input.TicketID,
		"link_id":       link.ID,
		"resource_kind": input.ResourceKind,
		"resource_id":   input.ResourceID,
	}); err != nil {
		return nil, err
	}
	return link, nil
}

func (r *PGRepository) RecordTicketClassification(ctx context.Context, input TicketClassificationInput, payload map[string]any) (*TicketClassification, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	actionID := newID("aiact")
	row := r.pool.QueryRow(ctx, `
INSERT INTO conversation_ai_actions (
	id, org_id, conversation_id, kind, status, payload, created_by, created_at, updated_at
) VALUES ($1, $2, $3, 'ticket.classification', $4, $5::jsonb, $6, NOW(), NOW())
RETURNING id, org_id, conversation_id, payload, created_at`,
		actionID, input.OrgID, input.ConversationID, input.Outcome, mustJSON(payload), createdBy(input.ActorUserID))
	var classification TicketClassification
	var payloadBytes []byte
	if err := row.Scan(&classification.ID, &classification.OrgID, &classification.ConversationID, &payloadBytes, &classification.CreatedAt); err != nil {
		return nil, err
	}
	_ = json.Unmarshal(payloadBytes, &classification.Payload)
	classification.Outcome = input.Outcome
	classification.Confidence = input.Confidence
	classification.Reason = input.Reason
	return &classification, nil
}

func (r *PGRepository) ListTicketViews(ctx context.Context, orgID string) ([]TicketView, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, name, scope, owner_user_id, team_id, visibility, filter, sort,
	group_by, sidebar_order, created_at, updated_at
FROM conversation_ticket_views
WHERE org_id = $1
ORDER BY sidebar_order ASC, name ASC`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	items := []TicketView{}
	for rows.Next() {
		item, err := scanTicketView(rows)
		if err != nil {
			return nil, err
		}
		items = append(items, *item)
	}
	return items, rows.Err()
}

func (r *PGRepository) CreateTicketView(ctx context.Context, input CreateTicketViewInput) (*TicketView, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	id := newID("view")
	row := r.pool.QueryRow(ctx, `
INSERT INTO conversation_ticket_views (
	id, org_id, name, scope, owner_user_id, team_id, visibility, filter, sort, group_by,
	sidebar_order, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9::jsonb, $10, $11, NOW(), NOW())
RETURNING id, org_id, name, scope, owner_user_id, team_id, visibility, filter, sort,
	group_by, sidebar_order, created_at, updated_at`,
		id, input.OrgID, input.Name, input.Scope, input.OwnerUserID, input.TeamID, input.Visibility,
		mustJSON(input.Filter), mustJSON(input.Sort), input.GroupBy, input.SidebarOrder)
	return scanTicketView(row)
}

func (r *PGRepository) UpdateTicketView(ctx context.Context, input UpdateTicketViewInput) (*TicketView, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	assignments := []string{}
	args := []any{input.OrgID, input.ID}
	add := func(column string, value any) {
		args = append(args, value)
		assignments = append(assignments, fmt.Sprintf("%s = $%d", column, len(args)))
	}
	if input.Name != nil {
		add("name", *input.Name)
	}
	if input.Scope != nil {
		add("scope", *input.Scope)
	}
	if input.OwnerUserID != nil {
		add("owner_user_id", *input.OwnerUserID)
	}
	if input.TeamID != nil {
		add("team_id", *input.TeamID)
	}
	if input.Visibility != nil {
		add("visibility", *input.Visibility)
	}
	if input.Filter != nil {
		args = append(args, mustJSON(*input.Filter))
		assignments = append(assignments, fmt.Sprintf("filter = $%d::jsonb", len(args)))
	}
	if input.Sort != nil {
		args = append(args, mustJSON(*input.Sort))
		assignments = append(assignments, fmt.Sprintf("sort = $%d::jsonb", len(args)))
	}
	if input.GroupBy != nil {
		add("group_by", *input.GroupBy)
	}
	if input.SidebarOrder != nil {
		add("sidebar_order", *input.SidebarOrder)
	}
	if len(assignments) == 0 {
		row := r.pool.QueryRow(ctx, `
SELECT id, org_id, name, scope, owner_user_id, team_id, visibility, filter, sort,
	group_by, sidebar_order, created_at, updated_at
FROM conversation_ticket_views
WHERE org_id = $1 AND id = $2`, input.OrgID, input.ID)
		return scanTicketView(row)
	}
	assignments = append(assignments, "updated_at = NOW()")
	row := r.pool.QueryRow(ctx, `
UPDATE conversation_ticket_views
SET `+strings.Join(assignments, ", ")+`
WHERE org_id = $1 AND id = $2
RETURNING id, org_id, name, scope, owner_user_id, team_id, visibility, filter, sort,
	group_by, sidebar_order, created_at, updated_at`, args...)
	item, err := scanTicketView(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return item, err
}

func (r *PGRepository) ListTicketMacros(ctx context.Context, orgID string) ([]TicketMacro, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, name, description, visibility, team_id, active, actions, conditions, created_at, updated_at
FROM conversation_ticket_macros
WHERE org_id = $1
ORDER BY active DESC, name ASC`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	items := []TicketMacro{}
	for rows.Next() {
		item, err := scanTicketMacro(rows)
		if err != nil {
			return nil, err
		}
		items = append(items, *item)
	}
	return items, rows.Err()
}

func (r *PGRepository) GetTicketMacro(ctx context.Context, orgID, macroID string) (*TicketMacro, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, `
SELECT id, org_id, name, description, visibility, team_id, active, actions, conditions, created_at, updated_at
FROM conversation_ticket_macros
WHERE org_id = $1 AND id = $2`, orgID, macroID)
	macro, err := scanTicketMacro(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return macro, err
}

func (r *PGRepository) CreateTicketMacro(ctx context.Context, input CreateTicketMacroInput) (*TicketMacro, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	id := newID("macro")
	row := r.pool.QueryRow(ctx, `
INSERT INTO conversation_ticket_macros (
	id, org_id, name, description, visibility, team_id, active, actions, conditions, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9::jsonb, NOW(), NOW())
RETURNING id, org_id, name, description, visibility, team_id, active, actions, conditions, created_at, updated_at`,
		id, input.OrgID, input.Name, input.Description, input.Visibility, input.TeamID, input.Active,
		mustJSON(input.Actions), mustJSON(input.Conditions))
	return scanTicketMacro(row)
}

func (r *PGRepository) UpdateTicketMacro(ctx context.Context, input UpdateTicketMacroInput) (*TicketMacro, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	assignments := []string{}
	args := []any{input.OrgID, input.ID}
	add := func(column string, value any) {
		args = append(args, value)
		assignments = append(assignments, fmt.Sprintf("%s = $%d", column, len(args)))
	}
	if input.Name != nil {
		add("name", *input.Name)
	}
	if input.Description != nil {
		add("description", *input.Description)
	}
	if input.Visibility != nil {
		add("visibility", *input.Visibility)
	}
	if input.TeamID != nil {
		add("team_id", *input.TeamID)
	}
	if input.Active != nil {
		add("active", *input.Active)
	}
	if input.Actions != nil {
		args = append(args, mustJSON(*input.Actions))
		assignments = append(assignments, fmt.Sprintf("actions = $%d::jsonb", len(args)))
	}
	if input.Conditions != nil {
		args = append(args, mustJSON(*input.Conditions))
		assignments = append(assignments, fmt.Sprintf("conditions = $%d::jsonb", len(args)))
	}
	if len(assignments) == 0 {
		return r.GetTicketMacro(ctx, input.OrgID, input.ID)
	}
	assignments = append(assignments, "updated_at = NOW()")
	row := r.pool.QueryRow(ctx, `
UPDATE conversation_ticket_macros
SET `+strings.Join(assignments, ", ")+`
WHERE org_id = $1 AND id = $2
RETURNING id, org_id, name, description, visibility, team_id, active, actions, conditions, created_at, updated_at`, args...)
	macro, err := scanTicketMacro(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return macro, err
}

func (r *PGRepository) RecordTicketMacroRun(ctx context.Context, input TicketMacroRunInput) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	_, err := r.pool.Exec(ctx, `
INSERT INTO conversation_ticket_macro_runs (id, org_id, ticket_id, macro_id, actor_user_id, actions, created_at)
VALUES ($1, $2, $3, $4, $5, $6::jsonb, NOW())`,
		newID("macrorun"), input.OrgID, input.TicketID, input.MacroID, input.ActorUserID, mustJSON(input.Actions))
	return err
}

func (r *PGRepository) ListTicketAutomationRules(ctx context.Context, orgID string) ([]TicketAutomationRule, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, name, event_name, active, conditions, actions, created_at, updated_at
FROM conversation_ticket_automation_rules
WHERE org_id = $1
ORDER BY active DESC, event_name ASC, name ASC`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	items := []TicketAutomationRule{}
	for rows.Next() {
		item, err := scanTicketAutomationRule(rows)
		if err != nil {
			return nil, err
		}
		items = append(items, *item)
	}
	return items, rows.Err()
}

func (r *PGRepository) CreateTicketAutomationRule(ctx context.Context, input CreateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	id := newID("rule")
	row := r.pool.QueryRow(ctx, `
INSERT INTO conversation_ticket_automation_rules (
	id, org_id, name, event_name, active, conditions, actions, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb, NOW(), NOW())
RETURNING id, org_id, name, event_name, active, conditions, actions, created_at, updated_at`,
		id, input.OrgID, input.Name, input.EventName, input.Active, mustJSON(input.Conditions), mustJSON(input.Actions))
	return scanTicketAutomationRule(row)
}

func (r *PGRepository) UpdateTicketAutomationRule(ctx context.Context, input UpdateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	assignments := []string{}
	args := []any{input.OrgID, input.ID}
	add := func(column string, value any) {
		args = append(args, value)
		assignments = append(assignments, fmt.Sprintf("%s = $%d", column, len(args)))
	}
	if input.Name != nil {
		add("name", *input.Name)
	}
	if input.EventName != nil {
		add("event_name", *input.EventName)
	}
	if input.Active != nil {
		add("active", *input.Active)
	}
	if input.Conditions != nil {
		args = append(args, mustJSON(*input.Conditions))
		assignments = append(assignments, fmt.Sprintf("conditions = $%d::jsonb", len(args)))
	}
	if input.Actions != nil {
		args = append(args, mustJSON(*input.Actions))
		assignments = append(assignments, fmt.Sprintf("actions = $%d::jsonb", len(args)))
	}
	if len(assignments) == 0 {
		row := r.pool.QueryRow(ctx, `
SELECT id, org_id, name, event_name, active, conditions, actions, created_at, updated_at
FROM conversation_ticket_automation_rules
WHERE org_id = $1 AND id = $2`, input.OrgID, input.ID)
		return scanTicketAutomationRule(row)
	}
	assignments = append(assignments, "updated_at = NOW()")
	row := r.pool.QueryRow(ctx, `
UPDATE conversation_ticket_automation_rules
SET `+strings.Join(assignments, ", ")+`
WHERE org_id = $1 AND id = $2
RETURNING id, org_id, name, event_name, active, conditions, actions, created_at, updated_at`, args...)
	item, err := scanTicketAutomationRule(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return item, err
}

func (r *PGRepository) ListSLAPolicies(ctx context.Context, orgID string) ([]SLAPolicy, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, name, active, conditions, calendar_ref, first_response_minutes,
	next_response_minutes, resolution_minutes, created_at, updated_at
FROM conversation_sla_policies
WHERE org_id = $1
ORDER BY active DESC, name ASC`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	items := []SLAPolicy{}
	for rows.Next() {
		item, err := scanSLAPolicy(rows)
		if err != nil {
			return nil, err
		}
		items = append(items, *item)
	}
	return items, rows.Err()
}

func (r *PGRepository) CreateSLAPolicy(ctx context.Context, input CreateSLAPolicyInput) (*SLAPolicy, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	id := newID("sla")
	row := r.pool.QueryRow(ctx, `
INSERT INTO conversation_sla_policies (
	id, org_id, name, active, conditions, calendar_ref, first_response_minutes,
	next_response_minutes, resolution_minutes, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7, $8, $9, NOW(), NOW())
RETURNING id, org_id, name, active, conditions, calendar_ref, first_response_minutes,
	next_response_minutes, resolution_minutes, created_at, updated_at`,
		id, input.OrgID, input.Name, input.Active, mustJSON(input.Conditions), input.CalendarRef,
		input.FirstResponseMinutes, input.NextResponseMinutes, input.ResolutionMinutes)
	return scanSLAPolicy(row)
}

func (r *PGRepository) UpdateSLAPolicy(ctx context.Context, input UpdateSLAPolicyInput) (*SLAPolicy, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	assignments := []string{}
	args := []any{input.OrgID, input.ID}
	add := func(column string, value any) {
		args = append(args, value)
		assignments = append(assignments, fmt.Sprintf("%s = $%d", column, len(args)))
	}
	if input.Name != nil {
		add("name", *input.Name)
	}
	if input.Active != nil {
		add("active", *input.Active)
	}
	if input.Conditions != nil {
		args = append(args, mustJSON(*input.Conditions))
		assignments = append(assignments, fmt.Sprintf("conditions = $%d::jsonb", len(args)))
	}
	if input.CalendarRef != nil {
		add("calendar_ref", *input.CalendarRef)
	}
	if input.FirstResponseMinutes != nil {
		add("first_response_minutes", *input.FirstResponseMinutes)
	}
	if input.NextResponseMinutes != nil {
		add("next_response_minutes", *input.NextResponseMinutes)
	}
	if input.ResolutionMinutes != nil {
		add("resolution_minutes", *input.ResolutionMinutes)
	}
	if len(assignments) == 0 {
		row := r.pool.QueryRow(ctx, `
SELECT id, org_id, name, active, conditions, calendar_ref, first_response_minutes,
	next_response_minutes, resolution_minutes, created_at, updated_at
FROM conversation_sla_policies
WHERE org_id = $1 AND id = $2`, input.OrgID, input.ID)
		return scanSLAPolicy(row)
	}
	assignments = append(assignments, "updated_at = NOW()")
	row := r.pool.QueryRow(ctx, `
UPDATE conversation_sla_policies
SET `+strings.Join(assignments, ", ")+`
WHERE org_id = $1 AND id = $2
RETURNING id, org_id, name, active, conditions, calendar_ref, first_response_minutes,
	next_response_minutes, resolution_minutes, created_at, updated_at`, args...)
	item, err := scanSLAPolicy(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return item, err
}

func (r *PGRepository) CreateTicketChecklist(ctx context.Context, input CreateTicketChecklistInput) (*TicketChecklist, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if _, err := r.GetTicket(ctx, input.OrgID, input.TicketID); err != nil {
		return nil, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()
	checklistID := newID("checklist")
	row := tx.QueryRow(ctx, `
INSERT INTO conversation_ticket_checklists (
	id, org_id, ticket_id, name, template_id, created_by_user_id, created_at, updated_at
) VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
RETURNING id, org_id, ticket_id, name, template_id, created_by_user_id, created_at, updated_at`,
		checklistID, input.OrgID, input.TicketID, input.Name, input.TemplateID, input.CreatedByUserID)
	checklist, err := scanTicketChecklist(row)
	if err != nil {
		return nil, err
	}
	for idx, label := range input.Items {
		if _, err := tx.Exec(ctx, `
INSERT INTO conversation_ticket_checklist_items (
	id, org_id, checklist_id, label, completed, position, created_at, updated_at
) VALUES ($1, $2, $3, $4, FALSE, $5, NOW(), NOW())`,
			newID("checkitem"), input.OrgID, checklistID, label, idx); err != nil {
			return nil, err
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true
	return r.getTicketChecklist(ctx, input.OrgID, checklist.ID)
}

func (r *PGRepository) UpdateTicketChecklistItem(ctx context.Context, input UpdateTicketChecklistItemInput) (*TicketChecklist, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	tag, err := r.pool.Exec(ctx, `
UPDATE conversation_ticket_checklist_items item
SET completed = $5, updated_at = NOW()
FROM conversation_ticket_checklists checklist
WHERE item.org_id = $1
  AND item.id = $4
  AND item.checklist_id = checklist.id
  AND checklist.id = $3
  AND checklist.ticket_id = $2`,
		input.OrgID, input.TicketID, input.ChecklistID, input.ItemID, input.Completed)
	if err != nil {
		return nil, err
	}
	if tag.RowsAffected() == 0 {
		return nil, ErrNotFound
	}
	return r.getTicketChecklist(ctx, input.OrgID, input.ChecklistID)
}

func (r *PGRepository) loadLinksForTickets(ctx context.Context, orgID string, tickets []Ticket) ([]Ticket, error) {
	if len(tickets) == 0 {
		return tickets, nil
	}
	ticketIndex := make(map[string]int, len(tickets))
	ticketIDs := make([]string, 0, len(tickets))
	for i := range tickets {
		tickets[i].LinkedResources = []TicketLinkedResource{}
		ticketIndex[tickets[i].ID] = i
		ticketIDs = append(ticketIDs, tickets[i].ID)
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, ticket_id, conversation_id, link_type, resource_kind, resource_id, resource_url,
	label, metadata, created_by_user_id, created_at
FROM conversation_linked_resources
WHERE org_id = $1 AND ticket_id = ANY($2)
ORDER BY created_at DESC, id DESC`, orgID, ticketIDs)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	for rows.Next() {
		link, err := scanTicketLinkedResource(rows)
		if err != nil {
			return nil, err
		}
		if index, ok := ticketIndex[link.TicketID]; ok {
			tickets[index].LinkedResources = append(tickets[index].LinkedResources, *link)
		}
	}
	return tickets, rows.Err()
}

func (r *PGRepository) loadChecklistsForTickets(ctx context.Context, orgID string, tickets []Ticket) ([]Ticket, error) {
	if len(tickets) == 0 {
		return tickets, nil
	}
	ticketIndex := make(map[string]int, len(tickets))
	ticketIDs := make([]string, 0, len(tickets))
	for i := range tickets {
		tickets[i].Checklists = []TicketChecklist{}
		ticketIndex[tickets[i].ID] = i
		ticketIDs = append(ticketIDs, tickets[i].ID)
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, ticket_id, name, template_id, created_by_user_id, created_at, updated_at
FROM conversation_ticket_checklists
WHERE org_id = $1 AND ticket_id = ANY($2)
ORDER BY created_at DESC, id DESC`, orgID, ticketIDs)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	checklistIDs := []string{}
	checklistIndex := map[string]struct {
		ticketIndex    int
		checklistIndex int
	}{}
	for rows.Next() {
		checklist, err := scanTicketChecklist(rows)
		if err != nil {
			return nil, err
		}
		if idx, ok := ticketIndex[checklist.TicketID]; ok {
			checklist.Items = []TicketChecklistItem{}
			tickets[idx].Checklists = append(tickets[idx].Checklists, *checklist)
			checklistIDs = append(checklistIDs, checklist.ID)
			checklistIndex[checklist.ID] = struct {
				ticketIndex    int
				checklistIndex int
			}{ticketIndex: idx, checklistIndex: len(tickets[idx].Checklists) - 1}
		}
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if len(checklistIDs) == 0 {
		return tickets, nil
	}
	itemRows, err := r.pool.Query(ctx, `
SELECT id, org_id, checklist_id, label, completed, position, created_at, updated_at
FROM conversation_ticket_checklist_items
WHERE org_id = $1 AND checklist_id = ANY($2)
ORDER BY checklist_id ASC, position ASC, created_at ASC`, orgID, checklistIDs)
	if err != nil {
		return nil, err
	}
	defer itemRows.Close()
	for itemRows.Next() {
		item, err := scanTicketChecklistItem(itemRows)
		if err != nil {
			return nil, err
		}
		if index, ok := checklistIndex[item.ChecklistID]; ok {
			tickets[index.ticketIndex].Checklists[index.checklistIndex].Items = append(
				tickets[index.ticketIndex].Checklists[index.checklistIndex].Items,
				*item,
			)
		}
	}
	return tickets, itemRows.Err()
}

func (r *PGRepository) getTicketChecklist(ctx context.Context, orgID, checklistID string) (*TicketChecklist, error) {
	row := r.pool.QueryRow(ctx, `
SELECT id, org_id, ticket_id, name, template_id, created_by_user_id, created_at, updated_at
FROM conversation_ticket_checklists
WHERE org_id = $1 AND id = $2`, orgID, checklistID)
	checklist, err := scanTicketChecklist(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, checklist_id, label, completed, position, created_at, updated_at
FROM conversation_ticket_checklist_items
WHERE org_id = $1 AND checklist_id = $2
ORDER BY position ASC, created_at ASC`, orgID, checklistID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	checklist.Items = []TicketChecklistItem{}
	for rows.Next() {
		item, err := scanTicketChecklistItem(rows)
		if err != nil {
			return nil, err
		}
		checklist.Items = append(checklist.Items, *item)
	}
	return checklist, rows.Err()
}

func (r *PGRepository) ensureConfigured() error {
	if r == nil || r.pool == nil {
		return fmt.Errorf("conversation repository is not configured")
	}
	return nil
}

func (r *PGRepository) ensureDefaultInbox(ctx context.Context, orgID, channel string) error {
	_, err := r.pool.Exec(ctx, `
INSERT INTO conversation_inboxes (id, org_id, name, channel)
VALUES ($1, $2, $3, $4)
ON CONFLICT (org_id, channel) DO NOTHING`, stableInboxID(orgID, channel), orgID, titleCase(channel), channel)
	return err
}

func (r *PGRepository) getSummary(ctx context.Context, orgID, conversationID string) (*ConversationSummary, error) {
	row := r.pool.QueryRow(ctx, `
SELECT
	c.id, c.org_id, c.inbox_id, c.title, c.status, c.priority, c.channel,
	c.provider, c.provider_thread_id, c.assignee_user_id, c.assignee_name,
	c.last_message_preview, c.last_message_at, c.created_at, c.updated_at,
	COALESCE(ct.id, ''), COALESCE(ct.name, ''), COALESCE(ct.email, ''), COALESCE(ct.phone, '')
FROM conversations c
LEFT JOIN conversation_contacts ct ON ct.id = c.contact_id
WHERE c.org_id = $1 AND c.id = $2`, orgID, conversationID)
	summary, err := scanConversationSummary(row)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	withTags, err := r.loadTagsForSummaries(ctx, []ConversationSummary{summary})
	if err != nil {
		return nil, err
	}
	return &withTags[0], nil
}

func (r *PGRepository) listMessages(ctx context.Context, orgID, conversationID string) ([]Message, error) {
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
	body_text, body_html, internal, provider, provider_message_id, provider_event_id, occurred_at, created_at
FROM conversation_messages
WHERE org_id = $1 AND conversation_id = $2
ORDER BY occurred_at ASC, id ASC`, orgID, conversationID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	messages := []Message{}
	for rows.Next() {
		message, err := scanMessage(rows)
		if err != nil {
			return nil, err
		}
		messages = append(messages, *message)
	}
	return messages, rows.Err()
}

func (r *PGRepository) loadTagsForSummaries(ctx context.Context, summaries []ConversationSummary) ([]ConversationSummary, error) {
	if len(summaries) == 0 {
		return summaries, nil
	}
	ids := make([]string, 0, len(summaries))
	positions := make(map[string]int, len(summaries))
	for idx, summary := range summaries {
		ids = append(ids, summary.ID)
		positions[summary.ID] = idx
	}
	rows, err := r.pool.Query(ctx, `
SELECT link.conversation_id, tag.name
FROM conversation_tag_links link
JOIN conversation_tags tag ON tag.id = link.tag_id
WHERE link.conversation_id = ANY($1)
ORDER BY tag.name`, ids)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var conversationID, tag string
		if err := rows.Scan(&conversationID, &tag); err != nil {
			return nil, err
		}
		if idx, ok := positions[conversationID]; ok {
			summaries[idx].Tags = append(summaries[idx].Tags, tag)
		}
	}
	return summaries, rows.Err()
}

type scanner interface {
	Scan(dest ...any) error
}

func scanConversationSummary(row scanner) (ConversationSummary, error) {
	var summary ConversationSummary
	if err := row.Scan(
		&summary.ID,
		&summary.OrgID,
		&summary.InboxID,
		&summary.Title,
		&summary.Status,
		&summary.Priority,
		&summary.Channel,
		&summary.Provider,
		&summary.ProviderThreadID,
		&summary.AssigneeUserID,
		&summary.AssigneeName,
		&summary.LastMessagePreview,
		&summary.LastMessageAt,
		&summary.CreatedAt,
		&summary.UpdatedAt,
		&summary.Contact.ID,
		&summary.Contact.Name,
		&summary.Contact.Email,
		&summary.Contact.Phone,
	); err != nil {
		return ConversationSummary{}, err
	}
	return summary, nil
}

func scanMessage(row scanner) (*Message, error) {
	var message Message
	if err := row.Scan(
		&message.ID,
		&message.OrgID,
		&message.ConversationID,
		&message.Direction,
		&message.SenderType,
		&message.SenderName,
		&message.SenderEmail,
		&message.BodyText,
		&message.BodyHTML,
		&message.Internal,
		&message.Provider,
		&message.ProviderMessageID,
		&message.ProviderEventID,
		&message.OccurredAt,
		&message.CreatedAt,
	); err != nil {
		return nil, err
	}
	return &message, nil
}

func ticketSelectSQL() string {
	return `
SELECT
	t.id, t.org_id, t.conversation_id, t.ticket_key, t.status, t.priority, t.severity,
	t.category, t.intent, t.assignee_user_id, t.assignee_name, t.team_id, t.team_name,
	t.due_at, t.source, t.ai_confidence, t.ai_reason, t.created_by, t.created_at, t.updated_at,
	t.waiting_since, t.last_customer_reply_at, t.first_response_at, t.resolved_at, t.snoozed_until,
	t.sla_policy_id, t.escalation_at, t.labels,
	c.id, c.org_id, c.inbox_id, c.title, c.status, c.priority, c.channel,
	c.provider, c.provider_thread_id, c.assignee_user_id, c.assignee_name,
	c.last_message_preview, c.last_message_at, c.created_at, c.updated_at,
	COALESCE(ct.id, ''), COALESCE(ct.name, ''), COALESCE(ct.email, ''), COALESCE(ct.phone, '')
FROM conversation_tickets t
JOIN conversations c ON c.id = t.conversation_id AND c.org_id = t.org_id
LEFT JOIN conversation_contacts ct ON ct.id = c.contact_id`
}

func scanTicketWithConversation(row scanner) (Ticket, error) {
	var ticket Ticket
	var conversation ConversationSummary
	if err := row.Scan(
		&ticket.ID,
		&ticket.OrgID,
		&ticket.ConversationID,
		&ticket.TicketKey,
		&ticket.Status,
		&ticket.Priority,
		&ticket.Severity,
		&ticket.Category,
		&ticket.Intent,
		&ticket.AssigneeUserID,
		&ticket.AssigneeName,
		&ticket.TeamID,
		&ticket.TeamName,
		&ticket.DueAt,
		&ticket.Source,
		&ticket.AIConfidence,
		&ticket.AIReason,
		&ticket.CreatedBy,
		&ticket.CreatedAt,
		&ticket.UpdatedAt,
		&ticket.WaitingSince,
		&ticket.LastCustomerReplyAt,
		&ticket.FirstResponseAt,
		&ticket.ResolvedAt,
		&ticket.SnoozedUntil,
		&ticket.SLAPolicyID,
		&ticket.EscalationAt,
		&ticket.Labels,
		&conversation.ID,
		&conversation.OrgID,
		&conversation.InboxID,
		&conversation.Title,
		&conversation.Status,
		&conversation.Priority,
		&conversation.Channel,
		&conversation.Provider,
		&conversation.ProviderThreadID,
		&conversation.AssigneeUserID,
		&conversation.AssigneeName,
		&conversation.LastMessagePreview,
		&conversation.LastMessageAt,
		&conversation.CreatedAt,
		&conversation.UpdatedAt,
		&conversation.Contact.ID,
		&conversation.Contact.Name,
		&conversation.Contact.Email,
		&conversation.Contact.Phone,
	); err != nil {
		return Ticket{}, err
	}
	ticket.Conversation = &conversation
	if ticket.Labels == nil {
		ticket.Labels = []string{}
	}
	ticket.SLAState = ticketSLAState(ticket)
	return ticket, nil
}

func scanTicketLinkedResource(row scanner) (*TicketLinkedResource, error) {
	var link TicketLinkedResource
	var metadataBytes []byte
	if err := row.Scan(
		&link.ID,
		&link.OrgID,
		&link.TicketID,
		&link.ConversationID,
		&link.LinkType,
		&link.ResourceKind,
		&link.ResourceID,
		&link.ResourceURL,
		&link.Label,
		&metadataBytes,
		&link.CreatedByUserID,
		&link.CreatedAt,
	); err != nil {
		return nil, err
	}
	_ = json.Unmarshal(metadataBytes, &link.Metadata)
	if link.Metadata == nil {
		link.Metadata = map[string]any{}
	}
	return &link, nil
}

func scanTicketView(row scanner) (*TicketView, error) {
	var item TicketView
	var filterBytes, sortBytes []byte
	if err := row.Scan(
		&item.ID,
		&item.OrgID,
		&item.Name,
		&item.Scope,
		&item.OwnerUserID,
		&item.TeamID,
		&item.Visibility,
		&filterBytes,
		&sortBytes,
		&item.GroupBy,
		&item.SidebarOrder,
		&item.CreatedAt,
		&item.UpdatedAt,
	); err != nil {
		return nil, err
	}
	item.Filter = jsonObject(filterBytes)
	item.Sort = jsonObject(sortBytes)
	return &item, nil
}

func scanTicketMacro(row scanner) (*TicketMacro, error) {
	var item TicketMacro
	var actionsBytes, conditionsBytes []byte
	if err := row.Scan(
		&item.ID,
		&item.OrgID,
		&item.Name,
		&item.Description,
		&item.Visibility,
		&item.TeamID,
		&item.Active,
		&actionsBytes,
		&conditionsBytes,
		&item.CreatedAt,
		&item.UpdatedAt,
	); err != nil {
		return nil, err
	}
	item.Actions = jsonObject(actionsBytes)
	item.Conditions = jsonObject(conditionsBytes)
	return &item, nil
}

func scanTicketAutomationRule(row scanner) (*TicketAutomationRule, error) {
	var item TicketAutomationRule
	var conditionsBytes, actionsBytes []byte
	if err := row.Scan(
		&item.ID,
		&item.OrgID,
		&item.Name,
		&item.EventName,
		&item.Active,
		&conditionsBytes,
		&actionsBytes,
		&item.CreatedAt,
		&item.UpdatedAt,
	); err != nil {
		return nil, err
	}
	item.Conditions = jsonObject(conditionsBytes)
	item.Actions = jsonObject(actionsBytes)
	return &item, nil
}

func scanSLAPolicy(row scanner) (*SLAPolicy, error) {
	var item SLAPolicy
	var conditionsBytes []byte
	if err := row.Scan(
		&item.ID,
		&item.OrgID,
		&item.Name,
		&item.Active,
		&conditionsBytes,
		&item.CalendarRef,
		&item.FirstResponseMinutes,
		&item.NextResponseMinutes,
		&item.ResolutionMinutes,
		&item.CreatedAt,
		&item.UpdatedAt,
	); err != nil {
		return nil, err
	}
	item.Conditions = jsonObject(conditionsBytes)
	return &item, nil
}

func scanTicketChecklist(row scanner) (*TicketChecklist, error) {
	var item TicketChecklist
	if err := row.Scan(
		&item.ID,
		&item.OrgID,
		&item.TicketID,
		&item.Name,
		&item.TemplateID,
		&item.CreatedByUserID,
		&item.CreatedAt,
		&item.UpdatedAt,
	); err != nil {
		return nil, err
	}
	item.Items = []TicketChecklistItem{}
	return &item, nil
}

func scanTicketChecklistItem(row scanner) (*TicketChecklistItem, error) {
	var item TicketChecklistItem
	if err := row.Scan(
		&item.ID,
		&item.OrgID,
		&item.ChecklistID,
		&item.Label,
		&item.Completed,
		&item.Position,
		&item.CreatedAt,
		&item.UpdatedAt,
	); err != nil {
		return nil, err
	}
	return &item, nil
}

func jsonObject(raw []byte) map[string]any {
	if len(raw) == 0 {
		return map[string]any{}
	}
	value := map[string]any{}
	if err := json.Unmarshal(raw, &value); err != nil {
		return map[string]any{}
	}
	return value
}

func ticketSLAState(ticket Ticket) string {
	if ticket.Status == "resolved" || ticket.Status == "closed" {
		return "ok"
	}
	now := time.Now().UTC()
	if ticket.EscalationAt != nil && !ticket.EscalationAt.After(now) {
		return "breached"
	}
	if ticket.DueAt == nil {
		return "ok"
	}
	if ticket.DueAt.Before(now) {
		return "breached"
	}
	if ticket.DueAt.Before(now.Add(24 * time.Hour)) {
		return "risk"
	}
	return "ok"
}

func findIdempotentResult(ctx context.Context, tx pgx.Tx, orgID, key string) (*StoredEventResult, error) {
	var conversationID, messageID string
	if err := tx.QueryRow(ctx, `
SELECT outcome_conversation_id, outcome_message_id
FROM conversation_idempotency_keys
WHERE org_id = $1 AND idempotency_key = $2`, orgID, key).Scan(&conversationID, &messageID); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}
	detail, err := detailFromTx(ctx, tx, orgID, conversationID)
	if err != nil {
		return nil, err
	}
	return &StoredEventResult{Detail: detail, Message: findMessage(detail.Messages, messageID), Created: false}, nil
}

func findByProviderMessage(ctx context.Context, tx pgx.Tx, event InboundEvent) (*StoredEventResult, error) {
	var conversationID, messageID string
	if err := tx.QueryRow(ctx, `
SELECT conversation_id, id
FROM conversation_messages
WHERE org_id = $1 AND provider = $2 AND provider_message_id = $3`, event.OrgID, event.Provider, event.ProviderMessageID).Scan(&conversationID, &messageID); err != nil {
		return nil, err
	}
	detail, err := detailFromTx(ctx, tx, event.OrgID, conversationID)
	if err != nil {
		return nil, err
	}
	return &StoredEventResult{Detail: detail, Message: findMessage(detail.Messages, messageID), Created: false}, nil
}

func detailFromTx(ctx context.Context, tx pgx.Tx, orgID, conversationID string) (*ConversationDetail, error) {
	row := tx.QueryRow(ctx, `
SELECT
	c.id, c.org_id, c.inbox_id, c.title, c.status, c.priority, c.channel,
	c.provider, c.provider_thread_id, c.assignee_user_id, c.assignee_name,
	c.last_message_preview, c.last_message_at, c.created_at, c.updated_at,
	COALESCE(ct.id, ''), COALESCE(ct.name, ''), COALESCE(ct.email, ''), COALESCE(ct.phone, '')
FROM conversations c
LEFT JOIN conversation_contacts ct ON ct.id = c.contact_id
WHERE c.org_id = $1 AND c.id = $2`, orgID, conversationID)
	summary, err := scanConversationSummary(row)
	if err != nil {
		return nil, err
	}
	rows, err := tx.Query(ctx, `
SELECT id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
	body_text, body_html, internal, provider, provider_message_id, provider_event_id, occurred_at, created_at
FROM conversation_messages
WHERE org_id = $1 AND conversation_id = $2
ORDER BY occurred_at ASC, id ASC`, orgID, conversationID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	messages := []Message{}
	for rows.Next() {
		message, err := scanMessage(rows)
		if err != nil {
			return nil, err
		}
		messages = append(messages, *message)
	}
	return &ConversationDetail{ConversationSummary: summary, Messages: messages}, rows.Err()
}

func findConversationByThread(ctx context.Context, tx pgx.Tx, event InboundEvent) (string, error) {
	if event.ProviderThreadID == "" {
		return "", ErrNotFound
	}
	var conversationID string
	if err := tx.QueryRow(ctx, `
SELECT conversation_id
FROM conversation_channel_thread_refs
WHERE org_id = $1 AND provider = $2 AND connection_id = $3 AND provider_thread_id = $4`,
		event.OrgID, event.Provider, event.ConnectionID, event.ProviderThreadID).Scan(&conversationID); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return "", ErrNotFound
		}
		return "", err
	}
	return conversationID, nil
}

func insertAuditAndEvent(ctx context.Context, tx pgx.Tx, orgID, conversationID, actorUserID, action string, payload map[string]any) error {
	payloadJSON := mustJSON(payload)
	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_audit_events (id, org_id, conversation_id, actor_user_id, action, payload)
VALUES ($1, $2, $3, $4, $5, $6::jsonb)`, newID("audit"), orgID, conversationID, actorUserID, action, payloadJSON); err != nil {
		return err
	}
	_, err := tx.Exec(ctx, `
INSERT INTO conversation_events (id, org_id, conversation_id, type, payload)
VALUES ($1, $2, $3, $4, $5::jsonb)`, newID("evt"), orgID, conversationID, action, payloadJSON)
	return err
}

func (r *PGRepository) insertSimpleAudit(ctx context.Context, orgID, conversationID, actorUserID, action string, payload map[string]any) error {
	_, err := r.pool.Exec(ctx, `
INSERT INTO conversation_audit_events (id, org_id, conversation_id, actor_user_id, action, payload)
VALUES ($1, $2, $3, $4, $5, $6::jsonb)`, newID("audit"), orgID, conversationID, actorUserID, action, mustJSON(payload))
	return err
}

func findMessage(messages []Message, id string) *Message {
	for idx := range messages {
		if messages[idx].ID == id {
			return &messages[idx]
		}
	}
	return nil
}

func preview(text, html string) string {
	value := strings.TrimSpace(text)
	if value == "" {
		value = strings.TrimSpace(html)
	}
	value = strings.Join(strings.Fields(value), " ")
	if len(value) > 240 {
		return value[:240]
	}
	return value
}

func titleCase(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return "Inbox"
	}
	return strings.ToUpper(value[:1]) + value[1:]
}

func hexLower(value string) string {
	sum := sha1.Sum([]byte(strings.ToLower(value)))
	return hex.EncodeToString(sum[:])[:16]
}

func stableTicketKey(orgID, conversationID string) string {
	return "TCK-" + strings.ToUpper(hexLower(orgID + ":" + conversationID)[:8])
}

func createdBy(actorUserID string) string {
	if strings.TrimSpace(actorUserID) == "" {
		return "model-plane"
	}
	return strings.TrimSpace(actorUserID)
}

func mustJSON(value map[string]any) string {
	if value == nil {
		return "{}"
	}
	bytes, err := json.Marshal(value)
	if err != nil {
		return "{}"
	}
	return string(bytes)
}
