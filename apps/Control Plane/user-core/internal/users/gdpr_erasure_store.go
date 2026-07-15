package users

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
)

type erasureOperationStore interface {
	BeginErasureOperation(context.Context, ErasureOperation) (ErasureOperation, error)
	GetErasureOperation(context.Context, string) (ErasureOperation, error)
	ClaimErasureOperation(context.Context, string) (ErasureOperation, bool, error)
	ClaimNextErasureOperation(context.Context) (ErasureOperation, bool, error)
	MarkErasureAuthCompleted(context.Context, string, int) error
	CompleteErasureLocalStage(context.Context, ErasureOperation) error
	MarkErasureAuditEnqueued(context.Context, string, int) error
	ClaimNextErasureFanout(context.Context, string) (ErasureFanoutChild, bool, error)
	CompleteErasureFanoutChild(context.Context, string, int) error
	FailErasureFanout(context.Context, string, int, time.Time, string, bool) error
	CompleteErasureFanout(context.Context, string, int) error
	FailErasureOperation(context.Context, string, int, time.Time, string) error
}

const maxErasureFanoutAttempts = 20

type ErasureFanoutChild struct {
	ChildEventID string
	OperationID  string
	OrgID        string
	Attempts     int
}

type erasureRow interface {
	Scan(...any) error
}

func scanErasureOperation(row erasureRow) (ErasureOperation, error) {
	var operation ErasureOperation
	err := row.Scan(
		&operation.OperationID,
		&operation.UserID,
		&operation.Mode,
		&operation.ActorID,
		&operation.ActorRole,
		&operation.OrgID,
		&operation.Attempts,
		&operation.NextAttemptAt,
		&operation.ProcessingAt,
		&operation.AuthCompletedAt,
		&operation.LocalCompletedAt,
		&operation.LocalUserDeleted,
		&operation.AuditEnqueuedAt,
		&operation.FanoutSnapshotAt,
		&operation.FanoutPublishedAt,
		&operation.CompletedAt,
		&operation.LastError,
		&operation.CreatedAt,
		&operation.UpdatedAt,
	)
	return operation, err
}

const erasureOperationColumns = `
	operation_id, user_id, mode, actor_id, actor_role, org_id, attempts,
	next_attempt_at, processing_at, auth_completed_at, local_completed_at,
	local_user_deleted, audit_enqueued_at, fanout_snapshot_at, fanout_published_at, completed_at,
	last_error, created_at, updated_at`

const qualifiedErasureOperationColumns = `
	operation.operation_id, operation.user_id, operation.mode,
	operation.actor_id, operation.actor_role, operation.org_id, operation.attempts,
	operation.next_attempt_at, operation.processing_at, operation.auth_completed_at,
	operation.local_completed_at, operation.local_user_deleted,
	operation.audit_enqueued_at, operation.fanout_snapshot_at, operation.fanout_published_at,
	operation.completed_at, operation.last_error, operation.created_at,
	operation.updated_at`

func (r *Repository) BeginErasureOperation(ctx context.Context, requested ErasureOperation) (ErasureOperation, error) {
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return ErasureOperation{}, fmt.Errorf("begin erasure snapshot: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	_, err = tx.Exec(ctx, `
		INSERT INTO user_erasure_operations (
			operation_id, user_id, mode, actor_id, actor_role, org_id
		) VALUES ($1, $2, $3, $4, $5, $6)
		ON CONFLICT (operation_id) DO NOTHING
	`, requested.OperationID, requested.UserID, requested.Mode, requested.ActorID, requested.ActorRole, requested.OrgID)
	if err != nil {
		return ErasureOperation{}, fmt.Errorf("persist erasure operation: %w", err)
	}

	operation, err := scanErasureOperation(tx.QueryRow(ctx, `
		SELECT `+erasureOperationColumns+`
		FROM user_erasure_operations WHERE operation_id=$1
		FOR UPDATE
	`, requested.OperationID))
	if err != nil {
		return ErasureOperation{}, err
	}
	if operation.UserID != requested.UserID || operation.Mode != requested.Mode {
		return ErasureOperation{}, fmt.Errorf("erasure operation identity collision")
	}
	if operation.FanoutSnapshotAt == nil {
		rows, queryErr := tx.Query(ctx, `
			SELECT DISTINCT org_id
			FROM (
				SELECT org_id FROM user_org_memberships
				WHERE user_id=$1 AND status='active'
				UNION ALL SELECT $2
			) affected
			WHERE length(trim(org_id)) BETWEEN 1 AND 255
			ORDER BY org_id
		`, requested.UserID, requested.OrgID)
		if queryErr != nil {
			return ErasureOperation{}, fmt.Errorf("snapshot erasure organizations: %w", queryErr)
		}
		orgIDs := make([]string, 0, 4)
		for rows.Next() {
			var orgID string
			if err := rows.Scan(&orgID); err != nil {
				rows.Close()
				return ErasureOperation{}, fmt.Errorf("read erasure organization: %w", err)
			}
			orgIDs = append(orgIDs, orgID)
		}
		if err := rows.Err(); err != nil {
			rows.Close()
			return ErasureOperation{}, fmt.Errorf("read erasure organizations: %w", err)
		}
		rows.Close()
		for _, orgID := range orgIDs {
			if _, err := tx.Exec(ctx, `
				INSERT INTO user_erasure_fanout (child_event_id, operation_id, org_id)
				VALUES ($1, $2, $3)
				ON CONFLICT (operation_id, org_id) DO NOTHING
			`, erasureFanoutChildID(operation.OperationID, orgID), operation.OperationID, orgID); err != nil {
				return ErasureOperation{}, fmt.Errorf("persist erasure organization snapshot: %w", err)
			}
		}
		if len(orgIDs) == 0 {
			return ErasureOperation{}, fmt.Errorf("erasure requires at least one affected organization")
		}
		if _, err := tx.Exec(ctx, `
			UPDATE user_erasure_operations SET fanout_snapshot_at=now(), updated_at=now()
			WHERE operation_id=$1 AND fanout_snapshot_at IS NULL
		`, operation.OperationID); err != nil {
			return ErasureOperation{}, fmt.Errorf("complete erasure organization snapshot: %w", err)
		}
		operation, err = scanErasureOperation(tx.QueryRow(ctx, `
			SELECT `+erasureOperationColumns+` FROM user_erasure_operations WHERE operation_id=$1
		`, operation.OperationID))
		if err != nil {
			return ErasureOperation{}, err
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return ErasureOperation{}, fmt.Errorf("commit erasure snapshot: %w", err)
	}
	return operation, nil
}

func (r *Repository) GetErasureOperation(ctx context.Context, operationID string) (ErasureOperation, error) {
	operation, err := scanErasureOperation(r.db.Pool.QueryRow(ctx, `
		SELECT `+erasureOperationColumns+`
		FROM user_erasure_operations
		WHERE operation_id = $1
	`, operationID))
	if errors.Is(err, pgx.ErrNoRows) {
		return ErasureOperation{}, fmt.Errorf("erasure operation not found")
	}
	return operation, err
}

func (r *Repository) ClaimErasureOperation(ctx context.Context, operationID string) (ErasureOperation, bool, error) {
	operation, err := scanErasureOperation(r.db.Pool.QueryRow(ctx, `
		UPDATE user_erasure_operations
		SET processing_at = now(), attempts = attempts + 1, updated_at = now()
		WHERE operation_id = $1
		  AND completed_at IS NULL
		  AND next_attempt_at <= now()
		  AND (processing_at IS NULL OR processing_at < now() - interval '1 minute')
		RETURNING `+erasureOperationColumns+`
	`, operationID))
	if errors.Is(err, pgx.ErrNoRows) {
		current, getErr := r.GetErasureOperation(ctx, operationID)
		return current, false, getErr
	}
	return operation, err == nil, err
}

func (r *Repository) ClaimNextErasureOperation(ctx context.Context) (ErasureOperation, bool, error) {
	operation, err := scanErasureOperation(r.db.Pool.QueryRow(ctx, `
		WITH candidate AS (
			SELECT operation_id
			FROM user_erasure_operations
			WHERE completed_at IS NULL
			  AND next_attempt_at <= now()
			  AND (processing_at IS NULL OR processing_at < now() - interval '1 minute')
			ORDER BY next_attempt_at, created_at
			FOR UPDATE SKIP LOCKED
			LIMIT 1
		)
		UPDATE user_erasure_operations AS operation
		SET processing_at = now(), attempts = operation.attempts + 1, updated_at = now()
		FROM candidate
		WHERE operation.operation_id = candidate.operation_id
		RETURNING `+qualifiedErasureOperationColumns+`
	`))
	if errors.Is(err, pgx.ErrNoRows) {
		return ErasureOperation{}, false, nil
	}
	return operation, err == nil, err
}

func (r *Repository) MarkErasureAuthCompleted(ctx context.Context, operationID string, attempts int) error {
	return r.updateErasureStage(ctx, `
		UPDATE user_erasure_operations
		SET auth_completed_at = COALESCE(auth_completed_at, now()), updated_at = now()
		WHERE operation_id = $1 AND attempts = $2 AND processing_at IS NOT NULL
	`, operationID, attempts, "Auth")
}

func (r *Repository) CompleteErasureLocalStage(ctx context.Context, operation ErasureOperation) error {
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin local erasure stage: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	localDeleted := false
	for _, statement := range []struct {
		name  string
		query string
	}{
		{name: "resource grants", query: `DELETE FROM resource_grants WHERE subject_type = 'user' AND subject_id = $1`},
		{name: "API keys", query: `DELETE FROM user_api_keys WHERE user_id = $1`},
		{name: "activity log", query: `DELETE FROM user_activity_log WHERE user_id = $1`},
	} {
		if _, err := tx.Exec(ctx, statement.query, operation.UserID); err != nil {
			return fmt.Errorf("delete user %s: %w", statement.name, err)
		}
	}

	switch operation.Mode {
	case ErasureModeHardDelete:
		if _, err := tx.Exec(ctx, `DELETE FROM user_org_memberships WHERE user_id = $1`, operation.UserID); err != nil {
			return fmt.Errorf("delete user organization memberships: %w", err)
		}
		command, err := tx.Exec(ctx, `DELETE FROM users WHERE id = $1`, operation.UserID)
		if err != nil {
			return fmt.Errorf("delete local user: %w", err)
		}
		localDeleted = command.RowsAffected() > 0
	case ErasureModeAnonymize:
		for _, statement := range []struct {
			name  string
			query string
		}{
			{name: "profile", query: `DELETE FROM user_profiles WHERE user_id = $1`},
			{name: "sessions", query: `DELETE FROM user_sessions WHERE user_id = $1`},
			{name: "activities", query: `DELETE FROM user_activities WHERE user_id = $1`},
			{name: "roles", query: `DELETE FROM user_roles WHERE user_id = $1`},
			{name: "devices", query: `DELETE FROM user_devices WHERE user_id = $1`},
			{name: "settings", query: `DELETE FROM user_settings WHERE user_id = $1`},
			{name: "provider accounts", query: `DELETE FROM provider_accounts WHERE user_id = $1`},
		} {
			if _, err := tx.Exec(ctx, statement.query, operation.UserID); err != nil {
				return fmt.Errorf("delete user %s: %w", statement.name, err)
			}
		}
		digest := sha256.Sum256([]byte(operation.UserID))
		anonymizedEmail := fmt.Sprintf("deleted_%x@anonymized.local", digest[:16])
		if _, err := tx.Exec(ctx, `
			UPDATE users
			SET email = $2, name = 'Deleted User', password_hash = '', avatar = '',
			    status = 'blocked', email_verified = false, onboarding_complete = false,
			    last_login_at = NULL, onboarding_step = NULL, onboarding_state = NULL,
			    onboarding_expires_at = NULL, onboarding_completed_at = NULL
			WHERE id = $1
		`, operation.UserID, anonymizedEmail); err != nil {
			return fmt.Errorf("anonymize local user: %w", err)
		}
	default:
		return fmt.Errorf("unsupported local erasure mode")
	}

	command, err := tx.Exec(ctx, `
		UPDATE user_erasure_operations
		SET local_completed_at = COALESCE(local_completed_at, now()),
		    local_user_deleted = local_user_deleted OR $3,
		    updated_at = now()
		WHERE operation_id = $1 AND attempts = $2 AND processing_at IS NOT NULL
	`, operation.OperationID, operation.Attempts, localDeleted)
	if err != nil {
		return fmt.Errorf("record local erasure stage: %w", err)
	}
	if command.RowsAffected() != 1 {
		return fmt.Errorf("local erasure stage lease was lost")
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("commit local erasure stage: %w", err)
	}
	return nil
}

func (r *Repository) MarkErasureAuditEnqueued(ctx context.Context, operationID string, attempts int) error {
	return r.updateErasureStage(ctx, `
		UPDATE user_erasure_operations
		SET audit_enqueued_at = COALESCE(audit_enqueued_at, now()), updated_at = now()
		WHERE operation_id = $1 AND attempts = $2 AND processing_at IS NOT NULL
	`, operationID, attempts, "audit")
}

func (r *Repository) ClaimNextErasureFanout(ctx context.Context, operationID string) (ErasureFanoutChild, bool, error) {
	child := ErasureFanoutChild{}
	err := r.db.Pool.QueryRow(ctx, `
		WITH candidate AS (
			SELECT child_event_id FROM user_erasure_fanout
			WHERE operation_id=$1 AND published_at IS NULL AND terminal_at IS NULL
			  AND next_attempt_at <= now()
			  AND (processing_at IS NULL OR processing_at < now() - interval '1 minute')
			ORDER BY next_attempt_at, created_at, child_event_id
			FOR UPDATE SKIP LOCKED LIMIT 1
		)
		UPDATE user_erasure_fanout child
		SET processing_at=now(), attempts=child.attempts+1, updated_at=now()
		FROM candidate WHERE child.child_event_id=candidate.child_event_id
		RETURNING child.child_event_id, child.operation_id, child.org_id, child.attempts
	`, operationID).Scan(&child.ChildEventID, &child.OperationID, &child.OrgID, &child.Attempts)
	if errors.Is(err, pgx.ErrNoRows) {
		return ErasureFanoutChild{}, false, nil
	}
	return child, err == nil, err
}

func (r *Repository) CompleteErasureFanoutChild(ctx context.Context, childEventID string, attempts int) error {
	command, err := r.db.Pool.Exec(ctx, `
		UPDATE user_erasure_fanout
		SET published_at=COALESCE(published_at, now()), processing_at=NULL,
		    last_error=NULL, updated_at=now()
		WHERE child_event_id=$1 AND attempts=$2 AND processing_at IS NOT NULL
	`, childEventID, attempts)
	if err == nil && command.RowsAffected() != 1 {
		return fmt.Errorf("erasure fan-out child completion lease was lost")
	}
	return err
}

func (r *Repository) FailErasureFanout(ctx context.Context, childEventID string, attempts int, nextAttempt time.Time, message string, terminal bool) error {
	command, err := r.db.Pool.Exec(ctx, `
		UPDATE user_erasure_fanout
		SET processing_at=NULL, next_attempt_at=$3, last_error=left($4, 2000),
		    terminal_at=CASE WHEN $5 THEN now() ELSE terminal_at END, updated_at=now()
		WHERE child_event_id=$1 AND attempts=$2 AND published_at IS NULL
	`, childEventID, attempts, nextAttempt, message, terminal)
	if err == nil && command.RowsAffected() != 1 {
		return fmt.Errorf("erasure fan-out child failure lease was lost")
	}
	return err
}

func (r *Repository) CompleteErasureFanout(ctx context.Context, operationID string, attempts int) error {
	return r.updateErasureStage(ctx, `
		UPDATE user_erasure_operations
		SET fanout_published_at = COALESCE(fanout_published_at, now()),
		    completed_at = COALESCE(completed_at, now()),
		    processing_at = NULL,
		    last_error = NULL,
		    updated_at = now()
		WHERE operation_id = $1 AND attempts = $2 AND processing_at IS NOT NULL
		  AND NOT EXISTS (
			SELECT 1 FROM user_erasure_fanout child
			WHERE child.operation_id=$1 AND child.published_at IS NULL
		  )
	`, operationID, attempts, "fan-out")
}

func (r *Repository) FailErasureOperation(ctx context.Context, operationID string, attempts int, nextAttempt time.Time, message string) error {
	command, err := r.db.Pool.Exec(ctx, `
		UPDATE user_erasure_operations
		SET processing_at = NULL,
		    next_attempt_at = $3,
		    last_error = left($4, 1000),
		    updated_at = now()
		WHERE operation_id = $1 AND attempts = $2 AND completed_at IS NULL
	`, operationID, attempts, nextAttempt, message)
	if err == nil && command.RowsAffected() != 1 {
		return fmt.Errorf("erasure operation failure lease was lost")
	}
	return err
}

func (r *Repository) updateErasureStage(ctx context.Context, query, operationID string, attempts int, stage string) error {
	command, err := r.db.Pool.Exec(ctx, query, operationID, attempts)
	if err == nil && command.RowsAffected() != 1 {
		return fmt.Errorf("%s erasure stage lease was lost", stage)
	}
	return err
}
