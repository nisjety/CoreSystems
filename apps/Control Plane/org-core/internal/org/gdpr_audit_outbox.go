package org

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

func newGDPRAuditEvent(
	orgID, subjectType, subjectID, actorID, actorRole, outcome string,
	occurredAt time.Time,
) (GDPRAuditEvent, error) {
	orgID = strings.TrimSpace(orgID)
	subjectType = strings.TrimSpace(subjectType)
	subjectID = strings.TrimSpace(subjectID)
	actorID = strings.TrimSpace(actorID)
	actorRole = strings.TrimSpace(actorRole)
	if orgID == "" || subjectID == "" || actorID == "" || actorRole == "" {
		return GDPRAuditEvent{}, fmt.Errorf("GDPR audit org, subject, actor, and role are required")
	}
	if subjectType != "organization" && subjectType != "organization_soft" {
		return GDPRAuditEvent{}, fmt.Errorf("unsupported GDPR audit subject type")
	}
	if outcome != "ok" && outcome != "error" {
		return GDPRAuditEvent{}, fmt.Errorf("unsupported GDPR audit outcome")
	}
	occurredAt = occurredAt.UTC()
	if occurredAt.IsZero() {
		return GDPRAuditEvent{}, fmt.Errorf("GDPR audit occurrence time is required")
	}
	identity := strings.Join([]string{
		subjectType, subjectID, outcome, occurredAt.Format(time.RFC3339Nano),
	}, "\x00")
	eventID := fmt.Sprintf("gdpr:org-core:%x", sha256.Sum256([]byte(identity)))
	return GDPRAuditEvent{
		EventID: eventID, Subject: GDPRErasureAuditSubject, OccurredAt: occurredAt,
		OrgID: orgID, UserID: actorID, ActorRole: actorRole,
		SubjectType: subjectType, SubjectID: subjectID, Outcome: outcome,
	}, nil
}

func (event GDPRAuditEvent) withReceipt(receipt json.RawMessage) (GDPRAuditEvent, error) {
	details := map[string]any{}
	if err := json.Unmarshal(receipt, &details); err != nil {
		return GDPRAuditEvent{}, fmt.Errorf("decode GDPR audit receipt: %w", err)
	}
	next := event
	next.Details = details
	return next, nil
}

func (event GDPRAuditEvent) Payload() (map[string]any, error) {
	canonical, err := newGDPRAuditEvent(
		event.OrgID, event.SubjectType, event.SubjectID,
		event.UserID, event.ActorRole, event.Outcome, event.OccurredAt,
	)
	if err != nil || event.EventID != canonical.EventID ||
		event.Subject != canonical.Subject || event.OrgID != canonical.OrgID ||
		event.UserID != canonical.UserID || event.ActorRole != canonical.ActorRole ||
		event.SubjectType != canonical.SubjectType || event.SubjectID != canonical.SubjectID {
		return nil, fmt.Errorf("invalid GDPR audit event identity or subject")
	}
	details := make(map[string]any, len(event.Details))
	for key, value := range event.Details {
		details[key] = value
	}
	return map[string]any{
		"event_id": canonical.EventID, "occurred_at": canonical.OccurredAt.Format(time.RFC3339Nano),
		"org_id": canonical.OrgID, "user_id": canonical.UserID, "actor_role": canonical.ActorRole,
		"plane": "control", "producer": "org-core", "event": "erasure",
		"subject":     canonical.SubjectType + ":" + canonical.SubjectID,
		"resource_id": canonical.SubjectID, "outcome": canonical.Outcome, "details": details,
	}, nil
}

func (r *Repository) executeGDPRAuditOperation(
	ctx context.Context,
	orgID, query string,
	event GDPRAuditEvent,
) (json.RawMessage, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin GDPR operation: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	var receipt []byte
	if err := tx.QueryRow(ctx, query, orgID).Scan(&receipt); err != nil {
		return nil, fmt.Errorf("execute GDPR operation: %w", err)
	}
	if err := validateDeletionReceipt(receipt); err != nil {
		return nil, err
	}
	completedEvent, err := event.withReceipt(receipt)
	if err != nil {
		return nil, err
	}
	if err := enqueueGDPRAuditEvent(ctx, tx, completedEvent); err != nil {
		return nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit GDPR operation and audit intent: %w", err)
	}
	return append(json.RawMessage(nil), receipt...), nil
}

type gdprAuditExecer interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
}

func enqueueGDPRAuditEvent(ctx context.Context, execer gdprAuditExecer, event GDPRAuditEvent) error {
	payload, err := event.Payload()
	if err != nil {
		return err
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("encode GDPR audit payload: %w", err)
	}
	if execer == nil {
		return fmt.Errorf("GDPR audit outbox store is unavailable")
	}
	if _, err := execer.Exec(ctx, `
INSERT INTO organization_gdpr_audit_outbox (event_id, org_id, subject, payload)
VALUES ($1, $2, $3, $4)`, event.EventID, event.OrgID, event.Subject, body); err != nil {
		return fmt.Errorf("record GDPR audit outbox: %w", err)
	}
	return nil
}

func (r *Repository) EnqueueGDPRAuditEvent(ctx context.Context, event GDPRAuditEvent) error {
	return enqueueGDPRAuditEvent(ctx, r.pool, event)
}

func (r *Repository) ClaimGDPRAuditOutbox(ctx context.Context, limit int) ([]GDPRAuditOutboxRow, error) {
	if limit < 1 || limit > 1000 {
		return nil, fmt.Errorf("GDPR audit outbox limit must be between 1 and 1000")
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin GDPR audit outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM organization_gdpr_audit_outbox
  WHERE published_at IS NULL
    AND dead_lettered_at IS NULL
    AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE organization_gdpr_audit_outbox o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.org_id, o.subject, o.payload, o.attempts`, limit, GDPRAuditMaxAttempts)
	if err != nil {
		return nil, fmt.Errorf("claim GDPR audit outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]GDPRAuditOutboxRow, 0)
	for rows.Next() {
		var row GDPRAuditOutboxRow
		var body []byte
		if err := rows.Scan(&row.EventID, &row.OrgID, &row.Subject, &body, &row.Attempts); err != nil {
			return nil, fmt.Errorf("scan GDPR audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.Payload); err != nil {
			return nil, fmt.Errorf("decode GDPR audit outbox payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate GDPR audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit GDPR audit outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) MarkGDPRAuditPublished(ctx context.Context, eventID string) error {
	result, err := r.pool.Exec(ctx, `
UPDATE organization_gdpr_audit_outbox
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark GDPR audit published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return fmt.Errorf("GDPR audit acknowledgement did not match a pending event")
	}
	return nil
}

func (r *Repository) MarkGDPRAuditPublishFailed(ctx context.Context, eventID string, publishErr error) (bool, error) {
	message := "unknown audit publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	var deadLettered bool
	err := r.pool.QueryRow(ctx, `
UPDATE organization_gdpr_audit_outbox
SET attempts = attempts + 1,
    processing_at = NULL,
    next_attempt_at = NOW() + LEAST(
      INTERVAL '5 minutes',
      INTERVAL '1 second' * power(2, LEAST(attempts, 8))
    ),
    dead_lettered_at = CASE WHEN attempts + 1 >= $2 THEN NOW() ELSE NULL END,
    last_error = $3,
    updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL
RETURNING dead_lettered_at IS NOT NULL`, eventID, GDPRAuditMaxAttempts, message).Scan(&deadLettered)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return false, fmt.Errorf("GDPR audit failure did not match a pending event")
		}
		return false, fmt.Errorf("record GDPR audit publish failure: %w", err)
	}
	return deadLettered, nil
}

func (r *Repository) GDPRAuditOutboxStatus(ctx context.Context) (GDPRAuditOutboxStatus, error) {
	var status GDPRAuditOutboxStatus
	err := r.pool.QueryRow(ctx, `
SELECT
  COUNT(*) FILTER (WHERE published_at IS NULL AND dead_lettered_at IS NULL),
  COUNT(*) FILTER (
    WHERE published_at IS NULL AND dead_lettered_at IS NULL AND processing_at IS NOT NULL
  ),
  COUNT(*) FILTER (WHERE dead_lettered_at IS NOT NULL),
  MIN(created_at) FILTER (WHERE published_at IS NULL AND dead_lettered_at IS NULL)
FROM organization_gdpr_audit_outbox`).Scan(
		&status.Pending, &status.InFlight, &status.DeadLettered, &status.OldestPendingAt,
	)
	if err != nil {
		return GDPRAuditOutboxStatus{}, fmt.Errorf("read GDPR audit outbox status: %w", err)
	}
	return status, nil
}

func (s *Service) FlushGDPRAuditOutbox(ctx context.Context, limit int) (GDPRAuditFlushResult, error) {
	if limit < 1 || limit > 1000 {
		return GDPRAuditFlushResult{}, fmt.Errorf("GDPR audit outbox limit must be between 1 and 1000")
	}
	if s.auditPublisher == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("GDPR audit publisher is unavailable")
	}
	if s.repo == nil || s.repo.pool == nil {
		return GDPRAuditFlushResult{}, fmt.Errorf("GDPR audit outbox repository is unavailable")
	}
	rows, err := s.repo.ClaimGDPRAuditOutbox(ctx, limit)
	if err != nil {
		return GDPRAuditFlushResult{}, err
	}
	result := GDPRAuditFlushResult{}
	var publishErrors error
	for _, row := range rows {
		if err := s.auditPublisher.PublishAudit(ctx, row.Subject, row.EventID, row.Payload); err != nil {
			deadLettered, markErr := s.repo.MarkGDPRAuditPublishFailed(ctx, row.EventID, err)
			if deadLettered {
				result.DeadLettered++
			}
			publishErrors = errors.Join(publishErrors, err, markErr)
			continue
		}
		if err := s.repo.MarkGDPRAuditPublished(ctx, row.EventID); err != nil {
			publishErrors = errors.Join(publishErrors, err)
			continue
		}
		result.Published++
	}
	return result, publishErrors
}

func (s *Service) GDPRAuditOutboxStatus(ctx context.Context) (GDPRAuditOutboxStatus, error) {
	if s.repo == nil || s.repo.pool == nil {
		return GDPRAuditOutboxStatus{}, fmt.Errorf("GDPR audit outbox repository is unavailable")
	}
	return s.repo.GDPRAuditOutboxStatus(ctx)
}
