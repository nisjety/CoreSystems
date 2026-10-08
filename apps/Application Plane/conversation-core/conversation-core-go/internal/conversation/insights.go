package conversation

import (
	"context"
	"fmt"
	"strings"
	"time"
)

// OutcomeReport reads retained owner records, independently of broker delivery.
// Ticket ownership is not attribution of the person who resolved a ticket.
type OutcomeReport struct {
	OrgID           string     `json:"org_id"`
	ActorUserID     string     `json:"actor_user_id,omitempty"`
	From            *time.Time `json:"from,omitempty"`
	To              *time.Time `json:"to,omitempty"`
	CheckedAt       time.Time  `json:"checked_at"`
	Source          string     `json:"source"`
	ResolvedTickets int64      `json:"resolved_tickets"`
	ApprovedReviews int64      `json:"approved_reviews"`
	RejectedReviews int64      `json:"rejected_reviews"`
}

type outcomeReader interface {
	ReadOutcomeReport(context.Context, string, string, *time.Time, *time.Time) (*OutcomeReport, error)
}

func (s *Service) ReadOutcomeReport(ctx context.Context, orgID, actor string, from, to *time.Time) (*OutcomeReport, error) {
	orgID, actor = strings.TrimSpace(orgID), strings.TrimSpace(actor)
	if orgID == "" || (from != nil && to != nil && !from.Before(*to)) {
		return nil, fmt.Errorf("%w: organization and a valid reporting window are required", ErrInvalidInput)
	}
	reader, ok := s.repository.(outcomeReader)
	if !ok {
		return nil, fmt.Errorf("outcome reporting is unavailable")
	}
	return reader.ReadOutcomeReport(ctx, orgID, actor, from, to)
}

func (r *PGRepository) ReadOutcomeReport(ctx context.Context, orgID, actor string, from, to *time.Time) (*OutcomeReport, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	result := &OutcomeReport{OrgID: orgID, ActorUserID: actor, From: from, To: to, Source: "conversation-core"}
	// A single statement uses one MVCC snapshot. No row limit, broker count,
	// prompt, message body, or customer information enters this report.
	err := r.pool.QueryRow(ctx, `SELECT
	 (SELECT COUNT(*) FROM conversation_tickets WHERE org_id = $1
	  AND status IN ('resolved','closed') AND resolved_at IS NOT NULL
	  AND ($2 = '' OR assignee_user_id = $2)
	  AND ($3::timestamptz IS NULL OR resolved_at >= $3)
	  AND ($4::timestamptz IS NULL OR resolved_at < $4)),
	 COUNT(*) FILTER (WHERE decision = 'approved'),
	 COUNT(*) FILTER (WHERE decision = 'rejected'), CURRENT_TIMESTAMP
	FROM conversation_ai_reviews WHERE org_id = $1
	 AND ($2 = '' OR reviewer_user_id = $2)
	 AND ($3::timestamptz IS NULL OR created_at >= $3)
	 AND ($4::timestamptz IS NULL OR created_at < $4)`, orgID, actor, from, to).
		Scan(&result.ResolvedTickets, &result.ApprovedReviews, &result.RejectedReviews, &result.CheckedAt)
	if err != nil {
		return nil, err
	}
	return result, nil
}
