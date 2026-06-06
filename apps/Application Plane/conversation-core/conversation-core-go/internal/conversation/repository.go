package conversation

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

type PGRepository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
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

	inboxID := stableInboxID(event.OrgID, "email")
	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_inboxes (id, org_id, name, channel)
VALUES ($1, $2, 'Email', 'email')
ON CONFLICT (org_id, channel) DO UPDATE SET updated_at = NOW()`, inboxID, event.OrgID); err != nil {
		return nil, err
	}

	contactID := stableContactID(event.OrgID, event.From.Email)
	if _, err := tx.Exec(ctx, `
INSERT INTO conversation_contacts (id, org_id, name, email, updated_at)
VALUES ($1, $2, $3, $4, NOW())
ON CONFLICT (org_id, lower(email)) WHERE email <> ''
DO UPDATE SET name = COALESCE(NULLIF(EXCLUDED.name, ''), conversation_contacts.name), updated_at = NOW()`,
		contactID, event.OrgID, event.From.Name, event.From.Email); err != nil {
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
) VALUES ($1, $2, $3, $4, $5, 'open', 'normal', 'email', $6, $7, $8, $9, $9, $9)`,
			conversationID, event.OrgID, inboxID, contactID, title, event.Provider, event.ProviderThreadID, preview(event.BodyText, event.BodyHTML), event.OccurredAt); err != nil {
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
		body_text, body_html, internal, occurred_at, created_at
	) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)
	RETURNING id, org_id, conversation_id, direction, sender_type, sender_name, sender_email,
		body_text, body_html, internal, provider, provider_message_id, provider_event_id, occurred_at, created_at
), updated AS (
	UPDATE conversations
	SET last_message_preview = $8, last_message_at = $11, updated_at = $11
	WHERE org_id = $2 AND id = $3
)
SELECT * FROM inserted`,
		messageID, input.OrgID, input.ConversationID, input.Direction, senderType, input.ActorName,
		input.ActorEmail, input.BodyText, input.BodyHTML, input.Internal, input.OccurredAt)
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
	if _, err := tx.Exec(ctx, `
UPDATE conversation_ai_actions
SET status = $3, reviewed_by = $4, reviewed_at = $5, updated_at = $5
WHERE org_id = $1 AND id = $2`, input.OrgID, input.AIActionID, input.Decision, input.ReviewerID, input.OccurredAt); err != nil {
		return err
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
