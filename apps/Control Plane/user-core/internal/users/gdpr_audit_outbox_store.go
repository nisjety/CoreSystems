package users

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
)

func (r *Repository) EnqueueAudit(ctx context.Context, row AuditOutboxRow) error {
	_, err := r.db.Pool.Exec(ctx, `
		INSERT INTO user_audit_outbox (event_id, subject, payload)
		VALUES ($1, $2, $3::jsonb)
		ON CONFLICT (event_id) DO NOTHING
	`, row.EventID, row.Subject, row.Payload)
	return err
}

func (r *Repository) ClaimAudit(ctx context.Context) (AuditOutboxRow, bool, error) {
	row := AuditOutboxRow{}
	err := r.db.Pool.QueryRow(ctx, `
		SELECT event_id, subject, payload, attempts
		FROM claim_user_audit_outbox()
	`).Scan(&row.EventID, &row.Subject, &row.Payload, &row.Attempts)
	if errors.Is(err, pgx.ErrNoRows) {
		return AuditOutboxRow{}, false, nil
	}
	return row, err == nil, err
}

func (r *Repository) CompleteAudit(ctx context.Context, eventID string, attempts int) error {
	command, err := r.db.Pool.Exec(ctx, `
		UPDATE user_audit_outbox
		SET published_at = now(), processing_at = NULL, last_error = NULL
		WHERE event_id = $1 AND attempts = $2 AND published_at IS NULL
	`, eventID, attempts)
	if err == nil && command.RowsAffected() != 1 {
		return fmt.Errorf("audit completion lease was lost")
	}
	return err
}

func (r *Repository) FailAudit(ctx context.Context, eventID string, attempts int, nextAttempt time.Time, message string, terminal bool) error {
	command, err := r.db.Pool.Exec(ctx, `
		UPDATE user_audit_outbox
		SET processing_at = NULL,
		    next_attempt_at = $3,
		    last_error = left($4, 2000),
		    terminal_at = CASE WHEN $5 THEN now() ELSE terminal_at END
		WHERE event_id = $1 AND attempts = $2 AND published_at IS NULL
	`, eventID, attempts, nextAttempt, message, terminal)
	if err == nil && command.RowsAffected() != 1 {
		return fmt.Errorf("audit failure lease was lost")
	}
	return err
}
