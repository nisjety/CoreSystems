package users

import (
	"context"
	"fmt"
	"strings"
	"time"
)

type GDPRDeliveryHealth struct {
	AuditPending   int64         `json:"audit_pending"`
	AuditTerminal  int64         `json:"audit_terminal"`
	FanoutPending  int64         `json:"fanout_pending"`
	FanoutTerminal int64         `json:"fanout_terminal"`
	OldestLag      time.Duration `json:"-"`
	OldestLagMS    int64         `json:"oldest_lag_ms"`
	Degraded       bool          `json:"degraded"`
}

func (r *Repository) GDPRDeliveryHealth(ctx context.Context, lagThreshold time.Duration) (GDPRDeliveryHealth, error) {
	var health GDPRDeliveryHealth
	var auditLagSeconds, fanoutLagSeconds float64
	err := r.db.Pool.QueryRow(ctx, `
		SELECT
			(SELECT count(*) FROM user_audit_outbox WHERE published_at IS NULL AND terminal_at IS NULL),
			(SELECT count(*) FROM user_audit_outbox WHERE terminal_at IS NOT NULL),
			COALESCE((SELECT extract(epoch FROM now()-min(created_at)) FROM user_audit_outbox WHERE published_at IS NULL), 0),
			(SELECT count(*) FROM user_erasure_fanout WHERE published_at IS NULL AND terminal_at IS NULL),
			(SELECT count(*) FROM user_erasure_fanout WHERE terminal_at IS NOT NULL),
			COALESCE((SELECT extract(epoch FROM now()-min(created_at)) FROM user_erasure_fanout WHERE published_at IS NULL), 0)
	`).Scan(
		&health.AuditPending, &health.AuditTerminal, &auditLagSeconds,
		&health.FanoutPending, &health.FanoutTerminal, &fanoutLagSeconds,
	)
	if err != nil {
		return GDPRDeliveryHealth{}, fmt.Errorf("read GDPR delivery health: %w", err)
	}
	oldestSeconds := auditLagSeconds
	if fanoutLagSeconds > oldestSeconds {
		oldestSeconds = fanoutLagSeconds
	}
	health.OldestLag = time.Duration(oldestSeconds * float64(time.Second))
	health.OldestLagMS = health.OldestLag.Milliseconds()
	health.Degraded = health.AuditTerminal > 0 || health.FanoutTerminal > 0 ||
		(lagThreshold > 0 && health.OldestLag >= lagThreshold)
	return health, nil
}

func (r *Repository) RequeueGDPRDeliveries(ctx context.Context, kind string, eventIDs []string) (int64, error) {
	if len(eventIDs) == 0 || len(eventIDs) > 100 {
		return 0, fmt.Errorf("requeue requires between 1 and 100 event IDs")
	}
	ids := make([]string, len(eventIDs))
	for index, eventID := range eventIDs {
		eventID = strings.TrimSpace(eventID)
		if eventID == "" || len(eventID) > 128 {
			return 0, fmt.Errorf("requeue event ID %d must contain 1-128 characters", index)
		}
		ids[index] = eventID
	}

	switch kind {
	case "audit":
		command, err := r.db.Pool.Exec(ctx, `
			UPDATE user_audit_outbox
			SET terminal_at=NULL, processing_at=NULL, next_attempt_at=now(), attempts=0,
			    requeue_count=requeue_count+1, requeued_at=now()
			WHERE event_id=ANY($1) AND terminal_at IS NOT NULL AND payload <> '{}'::jsonb
		`, ids)
		if err != nil {
			return 0, fmt.Errorf("requeue GDPR audit deliveries: %w", err)
		}
		return command.RowsAffected(), nil
	case "fanout":
		tx, err := r.db.Pool.Begin(ctx)
		if err != nil {
			return 0, fmt.Errorf("begin GDPR fanout requeue: %w", err)
		}
		defer func() { _ = tx.Rollback(ctx) }()
		command, err := tx.Exec(ctx, `
			UPDATE user_erasure_fanout
			SET terminal_at=NULL, processing_at=NULL, next_attempt_at=now(), attempts=0,
			    requeue_count=requeue_count+1, requeued_at=now(), updated_at=now()
			WHERE child_event_id=ANY($1) AND terminal_at IS NOT NULL AND published_at IS NULL
		`, ids)
		if err != nil {
			return 0, fmt.Errorf("requeue GDPR fanout deliveries: %w", err)
		}
		if command.RowsAffected() > 0 {
			if _, err := tx.Exec(ctx, `
				UPDATE user_erasure_operations operation
				SET next_attempt_at=now(), updated_at=now()
				WHERE EXISTS (
					SELECT 1 FROM user_erasure_fanout child
					WHERE child.operation_id=operation.operation_id
					  AND child.child_event_id=ANY($1)
				)
			`, ids); err != nil {
				return 0, fmt.Errorf("wake GDPR erasure operation: %w", err)
			}
		}
		if err := tx.Commit(ctx); err != nil {
			return 0, fmt.Errorf("commit GDPR fanout requeue: %w", err)
		}
		return command.RowsAffected(), nil
	default:
		return 0, fmt.Errorf("requeue kind must be audit or fanout")
	}
}

func (s *Service) GDPRDeliveryHealth(ctx context.Context) (GDPRDeliveryHealth, error) {
	if s == nil || s.repo == nil {
		return GDPRDeliveryHealth{}, fmt.Errorf("user repository is unavailable")
	}
	return s.repo.GDPRDeliveryHealth(ctx, 5*time.Minute)
}

func (s *Service) RequeueGDPRDeliveries(ctx context.Context, kind string, eventIDs []string) (int64, error) {
	if s == nil || s.repo == nil {
		return 0, fmt.Errorf("user repository is unavailable")
	}
	return s.repo.RequeueGDPRDeliveries(ctx, kind, eventIDs)
}
