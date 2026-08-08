package conversation

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"

	"github.com/jackc/pgx/v5"
)

// GetConversationFollow reads the caller's own follow preference. A missing
// preference is an ordinary not-found result; it must never be inferred from a
// different operator's row.
func (s *Service) GetConversationFollow(ctx context.Context, orgID, conversationID, userID string) (*ConversationFollow, error) {
	orgID, conversationID, userID = strings.TrimSpace(orgID), strings.TrimSpace(conversationID), strings.TrimSpace(userID)
	if orgID == "" || conversationID == "" || userID == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and user_id are required", ErrInvalidInput)
	}
	return s.repository.GetConversationFollow(ctx, orgID, conversationID, userID)
}

// FollowConversation is idempotent. It records a personal preference only;
// notification delivery is performed separately by the Application Plane.
func (s *Service) FollowConversation(ctx context.Context, orgID, conversationID, userID string) (*ConversationFollow, error) {
	input := ConversationFollowInput{
		OrgID:          strings.TrimSpace(orgID),
		ConversationID: strings.TrimSpace(conversationID),
		UserID:         strings.TrimSpace(userID),
	}
	if input.OrgID == "" || input.ConversationID == "" || input.UserID == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and user_id are required", ErrInvalidInput)
	}
	return s.repository.FollowConversation(ctx, input)
}

// UnfollowConversation is idempotent so callers can safely reconcile a stale
// UI state without learning whether another operator follows the conversation.
func (s *Service) UnfollowConversation(ctx context.Context, orgID, conversationID, userID string) error {
	orgID, conversationID, userID = strings.TrimSpace(orgID), strings.TrimSpace(conversationID), strings.TrimSpace(userID)
	if orgID == "" || conversationID == "" || userID == "" {
		return fmt.Errorf("%w: org_id, conversation_id, and user_id are required", ErrInvalidInput)
	}
	return s.repository.UnfollowConversation(ctx, orgID, conversationID, userID)
}

func (r *PGRepository) GetConversationFollow(ctx context.Context, orgID, conversationID, userID string) (*ConversationFollow, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	var follow ConversationFollow
	err := r.pool.QueryRow(ctx, `
SELECT org_id, conversation_id, user_id, created_at
FROM conversation_follows
WHERE org_id = $1 AND conversation_id = $2 AND user_id = $3`, orgID, conversationID, userID).Scan(
		&follow.OrgID, &follow.ConversationID, &follow.UserID, &follow.CreatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &follow, nil
}

func (r *PGRepository) FollowConversation(ctx context.Context, input ConversationFollowInput) (*ConversationFollow, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	var follow ConversationFollow
	err := r.pool.QueryRow(ctx, `
INSERT INTO conversation_follows (org_id, conversation_id, user_id)
SELECT $1, $2, $3
WHERE EXISTS (SELECT 1 FROM conversations WHERE org_id = $1 AND id = $2)
ON CONFLICT (org_id, conversation_id, user_id) DO UPDATE
SET user_id = EXCLUDED.user_id
RETURNING org_id, conversation_id, user_id, created_at`, input.OrgID, input.ConversationID, input.UserID).Scan(
		&follow.OrgID, &follow.ConversationID, &follow.UserID, &follow.CreatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &follow, nil
}

func (r *PGRepository) UnfollowConversation(ctx context.Context, orgID, conversationID, userID string) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	_, err := r.pool.Exec(ctx, `
DELETE FROM conversation_follows
WHERE org_id = $1 AND conversation_id = $2 AND user_id = $3`, orgID, conversationID, userID)
	return err
}

// ListConversationFollowerIDs is an internal fan-out projection. It returns
// only stable operator identifiers, in deterministic order, and never exposes
// follower records to a browser or a different tenant.
func (r *PGRepository) ListConversationFollowerIDs(ctx context.Context, orgID, conversationID string) ([]string, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT user_id
FROM conversation_follows
WHERE org_id = $1 AND conversation_id = $2
ORDER BY user_id ASC`, orgID, conversationID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	ids := make([]string, 0)
	for rows.Next() {
		var userID string
		if err := rows.Scan(&userID); err != nil {
			return nil, err
		}
		ids = append(ids, userID)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	sort.Strings(ids)
	return ids, nil
}
