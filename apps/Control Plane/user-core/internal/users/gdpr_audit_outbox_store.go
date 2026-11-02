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

// EnqueueAuditTx inserts an audit outbox row on the caller's transaction, so a
// state change and its audit intent commit or roll back together (WS03-S1).
// It validates the row exactly like the dispatching path.
func (r *Repository) EnqueueAuditTx(ctx context.Context, tx pgx.Tx, row AuditOutboxRow) error {
	if err := validateAuditOutboxRow(row); err != nil {
		return err
	}
	_, err := tx.Exec(ctx, `
		INSERT INTO user_audit_outbox (event_id, subject, payload)
		VALUES ($1, $2, $3::jsonb)
		ON CONFLICT (event_id) DO NOTHING
	`, row.EventID, row.Subject, row.Payload)
	return err
}

// SpaceAuditEnqueuer adapts Repository to the one-method interface
// internal/spaces uses for its in-transaction audit insert, so internal/spaces
// imports nothing from internal/users.
type SpaceAuditEnqueuer struct{ repo *Repository }

// NewSpaceAuditEnqueuer wraps a Repository for internal/spaces.
func NewSpaceAuditEnqueuer(repo *Repository) *SpaceAuditEnqueuer {
	return &SpaceAuditEnqueuer{repo: repo}
}

// EnqueueAuditTx implements spaces.PolicyAuditOutbox.
func (e *SpaceAuditEnqueuer) EnqueueAuditTx(ctx context.Context, tx pgx.Tx, eventID, subject string, payload []byte) error {
	if e == nil || e.repo == nil {
		return errors.New("audit outbox store is unavailable")
	}
	return e.repo.EnqueueAuditTx(ctx, tx, AuditOutboxRow{EventID: eventID, Subject: subject, Payload: payload})
}

// EnqueueAudit implements spaces.DecisionAuditOutbox (WS05-S5): a Space decision
// refusal or issue has no state change to join, so the row is inserted alone.
// It validates the row exactly like the other enqueue paths.
func (e *SpaceAuditEnqueuer) EnqueueAudit(ctx context.Context, eventID, subject string, payload []byte) error {
	if e == nil || e.repo == nil {
		return errors.New("audit outbox store is unavailable")
	}
	row := AuditOutboxRow{EventID: eventID, Subject: subject, Payload: payload}
	if err := validateAuditOutboxRow(row); err != nil {
		return err
	}
	return e.repo.EnqueueAudit(ctx, row)
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
