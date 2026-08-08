package conversation

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

const DraftLeaseTTL = time.Minute

func (s *Service) GetDraftLease(ctx context.Context, orgID, conversationID string) (*DraftLease, error) {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	return s.repository.GetDraftLease(ctx, strings.TrimSpace(orgID), strings.TrimSpace(conversationID))
}

func (s *Service) ClaimDraftLease(ctx context.Context, orgID, conversationID, userID string) (*DraftLease, error) {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || strings.TrimSpace(userID) == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and user_id are required", ErrInvalidInput)
	}
	return s.repository.ClaimDraftLease(ctx, DraftLeaseClaimInput{OrgID: strings.TrimSpace(orgID), ConversationID: strings.TrimSpace(conversationID), UserID: strings.TrimSpace(userID), ExpiresAt: s.now().UTC().Add(DraftLeaseTTL)})
}

func (s *Service) ReleaseDraftLease(ctx context.Context, orgID, conversationID, userID string) error {
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || strings.TrimSpace(userID) == "" {
		return fmt.Errorf("%w: org_id, conversation_id, and user_id are required", ErrInvalidInput)
	}
	return s.repository.ReleaseDraftLease(ctx, strings.TrimSpace(orgID), strings.TrimSpace(conversationID), strings.TrimSpace(userID))
}

func (r *PGRepository) GetDraftLease(ctx context.Context, orgID, conversationID string) (*DraftLease, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	var lease DraftLease
	err := r.pool.QueryRow(ctx, `SELECT org_id, conversation_id, user_id, expires_at, updated_at FROM conversation_draft_leases WHERE org_id = $1 AND conversation_id = $2 AND expires_at > NOW()`, orgID, conversationID).Scan(&lease.OrgID, &lease.ConversationID, &lease.UserID, &lease.ExpiresAt, &lease.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &lease, nil
}

func (r *PGRepository) ClaimDraftLease(ctx context.Context, input DraftLeaseClaimInput) (*DraftLease, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	var lease DraftLease
	err := r.pool.QueryRow(ctx, `
INSERT INTO conversation_draft_leases (org_id, conversation_id, user_id, expires_at)
SELECT $1, $2, $3, $4 WHERE EXISTS (SELECT 1 FROM conversations WHERE org_id = $1 AND id = $2)
ON CONFLICT (org_id, conversation_id) DO UPDATE SET user_id = EXCLUDED.user_id, expires_at = EXCLUDED.expires_at, updated_at = NOW()
WHERE conversation_draft_leases.user_id = EXCLUDED.user_id OR conversation_draft_leases.expires_at <= NOW()
RETURNING org_id, conversation_id, user_id, expires_at, updated_at`, input.OrgID, input.ConversationID, input.UserID, input.ExpiresAt).Scan(&lease.OrgID, &lease.ConversationID, &lease.UserID, &lease.ExpiresAt, &lease.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		if _, lookupErr := r.GetDraftLease(ctx, input.OrgID, input.ConversationID); lookupErr == nil {
			return nil, ErrConflict
		}
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return &lease, nil
}

func (r *PGRepository) ReleaseDraftLease(ctx context.Context, orgID, conversationID, userID string) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	_, err := r.pool.Exec(ctx, `DELETE FROM conversation_draft_leases WHERE org_id = $1 AND conversation_id = $2 AND user_id = $3`, orgID, conversationID, userID)
	return err
}
