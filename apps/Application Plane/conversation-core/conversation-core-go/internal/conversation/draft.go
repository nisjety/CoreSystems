package conversation

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"

	"github.com/jackc/pgx/v5"
)

const maxConversationDraftRunes = 8000

func (s *Service) GetConversationDraft(ctx context.Context, orgID, conversationID, userID string) (*ConversationDraft, error) {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || strings.TrimSpace(userID) == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and user_id are required", ErrInvalidInput)
	}
	return s.repository.GetConversationDraft(ctx, strings.TrimSpace(orgID), strings.TrimSpace(conversationID), strings.TrimSpace(userID))
}

func (s *Service) UpsertConversationDraft(ctx context.Context, input ConversationDraftInput) (*ConversationDraft, error) {
	input.OrgID, input.ConversationID, input.UserID = strings.TrimSpace(input.OrgID), strings.TrimSpace(input.ConversationID), strings.TrimSpace(input.UserID)
	input.BodyText = strings.TrimSpace(input.BodyText)
	if input.OrgID == "" || input.ConversationID == "" || input.UserID == "" || input.BodyText == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, user_id, and body_text are required", ErrInvalidInput)
	}
	if utf8.RuneCountInString(input.BodyText) > maxConversationDraftRunes {
		return nil, fmt.Errorf("%w: draft body_text exceeds %d characters", ErrInvalidInput, maxConversationDraftRunes)
	}
	return s.repository.UpsertConversationDraft(ctx, input)
}

func (s *Service) DeleteConversationDraft(ctx context.Context, orgID, conversationID, userID string) error {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || strings.TrimSpace(userID) == "" {
		return fmt.Errorf("%w: org_id, conversation_id, and user_id are required", ErrInvalidInput)
	}
	return s.repository.DeleteConversationDraft(ctx, strings.TrimSpace(orgID), strings.TrimSpace(conversationID), strings.TrimSpace(userID))
}

func (r *PGRepository) GetConversationDraft(ctx context.Context, orgID, conversationID, userID string) (*ConversationDraft, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	var draft ConversationDraft
	err := r.pool.QueryRow(ctx, `SELECT org_id, conversation_id, user_id, body_text, internal, updated_at FROM conversation_drafts WHERE org_id=$1 AND conversation_id=$2 AND user_id=$3`, orgID, conversationID, userID).Scan(&draft.OrgID, &draft.ConversationID, &draft.UserID, &draft.BodyText, &draft.Internal, &draft.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &draft, nil
}

func (r *PGRepository) UpsertConversationDraft(ctx context.Context, input ConversationDraftInput) (*ConversationDraft, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	var draft ConversationDraft
	err := r.pool.QueryRow(ctx, `INSERT INTO conversation_drafts (org_id, conversation_id, user_id, body_text, internal) SELECT $1,$2,$3,$4,$5 WHERE EXISTS (SELECT 1 FROM conversations WHERE org_id=$1 AND id=$2) ON CONFLICT (org_id, conversation_id, user_id) DO UPDATE SET body_text=EXCLUDED.body_text, internal=EXCLUDED.internal, updated_at=NOW() RETURNING org_id, conversation_id, user_id, body_text, internal, updated_at`, input.OrgID, input.ConversationID, input.UserID, input.BodyText, input.Internal).Scan(&draft.OrgID, &draft.ConversationID, &draft.UserID, &draft.BodyText, &draft.Internal, &draft.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &draft, nil
}

func (r *PGRepository) DeleteConversationDraft(ctx context.Context, orgID, conversationID, userID string) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	_, err := r.pool.Exec(ctx, `DELETE FROM conversation_drafts WHERE org_id=$1 AND conversation_id=$2 AND user_id=$3`, orgID, conversationID, userID)
	return err
}

// PurgeConversationDraftsByOrg removes only personal recovery drafts for an
// organization after Control Plane enables interactive Zero Data Retention.
// It deliberately leaves conversations, messages, tickets, and audit history
// intact. A plain org-scoped DELETE makes at-least-once event delivery safe:
// any redelivery simply deletes zero rows.
func (r *PGRepository) PurgeConversationDraftsByOrg(ctx context.Context, orgID string) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	_, err := r.pool.Exec(ctx, `DELETE FROM conversation_drafts WHERE org_id = $1`, orgID)
	return err
}
