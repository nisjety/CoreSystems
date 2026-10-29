package conversation

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
)

func (s *Service) GetConversationCSATPreference(ctx context.Context, orgID, conversationID string) (*CSATPreference, error) {
	orgID, conversationID = strings.TrimSpace(orgID), strings.TrimSpace(conversationID)
	if orgID == "" || conversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	return s.repository.GetConversationCSATPreference(ctx, orgID, conversationID)
}

func (s *Service) SetConversationCSATPreference(ctx context.Context, input CSATPreferenceInput) (*CSATPreference, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.ConversationID == "" || input.ActorUserID == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and actor_user_id are required", ErrInvalidInput)
	}
	return s.repository.SetConversationCSATPreference(ctx, input)
}

// GetTicketCSATOutcome returns a recorded rating, if the customer has supplied
// one. A missing outcome is intentionally not synthesised as a score or a
// survey state: current deployments may capture consent without delivery
// authority, and reporting must remain truthful about that distinction.
func (s *Service) GetTicketCSATOutcome(ctx context.Context, orgID, ticketID string) (*TicketCSATOutcome, error) {
	orgID, ticketID = strings.TrimSpace(orgID), strings.TrimSpace(ticketID)
	if orgID == "" || ticketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	return s.repository.GetTicketCSATOutcome(ctx, orgID, ticketID)
}

// RecordTicketCSATOutcome records the score an operator received from a
// customer. It never sends a message, creates an invitation, or invents a
// response-rate denominator. The support contact must be explicitly opted in
// and the linked ticket must already be terminal.
func (s *Service) RecordTicketCSATOutcome(ctx context.Context, input TicketCSATOutcomeInput) (*TicketCSATOutcome, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.RecordedBy = strings.TrimSpace(input.RecordedBy)
	if input.OrgID == "" || input.TicketID == "" || input.RecordedBy == "" || input.Score < 1 || input.Score > 5 {
		return nil, fmt.Errorf("%w: org_id, ticket_id, recorded_by, and a score from 1 to 5 are required", ErrInvalidInput)
	}
	ticket, err := s.repository.GetTicket(ctx, input.OrgID, input.TicketID)
	if err != nil {
		return nil, err
	}
	if ticket.Status != "resolved" && ticket.Status != "closed" {
		return nil, fmt.Errorf("%w: customer satisfaction can be recorded only after a ticket is resolved", ErrInvalidInput)
	}
	preference, err := s.repository.GetConversationCSATPreference(ctx, input.OrgID, ticket.ConversationID)
	if err != nil {
		return nil, err
	}
	if !preference.OptedIn {
		return nil, fmt.Errorf("%w: the support contact has not consented to feedback", ErrInvalidInput)
	}
	return s.repository.UpsertTicketCSATOutcome(ctx, input)
}

func (s *Service) GetCSATScorecard(ctx context.Context, orgID string) (*CSATScorecard, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.GetCSATScorecard(ctx, orgID)
}

func (r *PGRepository) GetConversationCSATPreference(ctx context.Context, orgID, conversationID string) (*CSATPreference, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	preference := &CSATPreference{OrgID: orgID, ConversationID: conversationID}
	err := r.pool.QueryRow(ctx, `
SELECT c.contact_id, COALESCE(p.opted_in, FALSE), COALESCE(p.updated_by, ''), p.updated_at
FROM conversations c
LEFT JOIN conversation_csat_preferences p ON p.org_id = c.org_id AND p.contact_id = c.contact_id
WHERE c.org_id = $1 AND c.id = $2 AND c.contact_id IS NOT NULL`, orgID, conversationID).Scan(
		&preference.ContactID, &preference.OptedIn, &preference.UpdatedBy, &preference.UpdatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return preference, nil
}

func (r *PGRepository) SetConversationCSATPreference(ctx context.Context, input CSATPreferenceInput) (*CSATPreference, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	preference := &CSATPreference{OrgID: input.OrgID, ConversationID: input.ConversationID}
	err := r.pool.QueryRow(ctx, `
WITH target AS (
  SELECT contact_id FROM conversations WHERE org_id = $1 AND id = $2 AND contact_id IS NOT NULL
), updated AS (
  INSERT INTO conversation_csat_preferences (org_id, contact_id, opted_in, updated_by)
  SELECT $1, contact_id, $3, $4 FROM target
  ON CONFLICT (org_id, contact_id) DO UPDATE SET opted_in = EXCLUDED.opted_in, updated_by = EXCLUDED.updated_by, updated_at = NOW()
  RETURNING contact_id, opted_in, updated_by, updated_at
)
SELECT contact_id, opted_in, updated_by, updated_at FROM updated`, input.OrgID, input.ConversationID, input.OptedIn, input.ActorUserID).Scan(
		&preference.ContactID, &preference.OptedIn, &preference.UpdatedBy, &preference.UpdatedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return preference, nil
}

func (r *PGRepository) GetTicketCSATOutcome(ctx context.Context, orgID, ticketID string) (*TicketCSATOutcome, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	outcome := &TicketCSATOutcome{OrgID: orgID, TicketID: ticketID}
	err := r.pool.QueryRow(ctx, `
SELECT org_id, ticket_id, conversation_id, score, recorded_by, recorded_at
FROM ticket_csat_outcomes
WHERE org_id = $1 AND ticket_id = $2`, orgID, ticketID).Scan(
		&outcome.OrgID, &outcome.TicketID, &outcome.ConversationID, &outcome.Score, &outcome.RecordedBy, &outcome.RecordedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return outcome, nil
}

func (r *PGRepository) UpsertTicketCSATOutcome(ctx context.Context, input TicketCSATOutcomeInput) (*TicketCSATOutcome, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	outcome := &TicketCSATOutcome{OrgID: input.OrgID, TicketID: input.TicketID}
	err := r.pool.QueryRow(ctx, `
WITH ticket AS (
  SELECT id, conversation_id
  FROM conversation_tickets
  WHERE org_id = $1 AND id = $2
)
INSERT INTO ticket_csat_outcomes (org_id, ticket_id, conversation_id, score, recorded_by)
SELECT $1, id, conversation_id, $3, $4 FROM ticket
ON CONFLICT (org_id, ticket_id) DO UPDATE
SET score = EXCLUDED.score, recorded_by = EXCLUDED.recorded_by, recorded_at = NOW()
RETURNING org_id, ticket_id, conversation_id, score, recorded_by, recorded_at`, input.OrgID, input.TicketID, input.Score, input.RecordedBy).Scan(
		&outcome.OrgID, &outcome.TicketID, &outcome.ConversationID, &outcome.Score, &outcome.RecordedBy, &outcome.RecordedAt,
	)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	return outcome, nil
}

func (r *PGRepository) GetCSATScorecard(ctx context.Context, orgID string) (*CSATScorecard, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	result := &CSATScorecard{}
	var average, positiveRate *float64
	err := r.pool.QueryRow(ctx, `
SELECT
  COUNT(*)::int,
  COUNT(*) FILTER (WHERE score >= 4)::int,
  AVG(score)::float8,
  CASE WHEN COUNT(*) = 0 THEN NULL ELSE (COUNT(*) FILTER (WHERE score >= 4))::float8 / COUNT(*)::float8 END
FROM ticket_csat_outcomes
WHERE org_id = $1`, orgID).Scan(&result.RatedTickets, &result.PositiveRatings, &average, &positiveRate)
	if err != nil {
		return nil, err
	}
	result.AverageScore, result.PositiveRate = average, positiveRate
	return result, nil
}
