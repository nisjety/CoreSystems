package retention

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// Repository stores the policy, the holds, their history and the audit intent.
// Every write and read runs inside WithOrgScope, so the row-level security of
// migration 031 is a second tenant filter behind the org_id predicates; only the
// outbox publisher runs on the owner pool, as the other org-core outboxes do.
type Repository struct {
	db *database.DB
}

func NewRepository(db *database.DB) *Repository {
	return &Repository{db: db}
}

// postgresForeignKeyViolation is SQLSTATE 23503.
const postgresForeignKeyViolation = "23503"

func (r *Repository) ready() error {
	if r == nil || r.db == nil {
		return errors.New("retention repository not configured")
	}
	return nil
}

// GetPolicy returns the organisation's policy; revision 0 and keep when none was
// ever written.
func (r *Repository) GetPolicy(ctx context.Context, orgID string) (*Policy, error) {
	if err := r.ready(); err != nil {
		return nil, err
	}
	var policy *Policy
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		loaded, err := loadPolicy(ctx, tx, orgID, false)
		policy = loaded
		return err
	})
	if err != nil {
		return nil, err
	}
	return policy, nil
}

// loadPolicy reads the policy row, FOR UPDATE when a writer is about to replace it,
// so two writers with one expected revision cannot both win.
func loadPolicy(ctx context.Context, tx pgx.Tx, orgID string, forUpdate bool) (*Policy, error) {
	query := `SELECT thread_days, revision, updated_by, updated_at FROM org_retention_policies WHERE org_id = $1`
	if forUpdate {
		query += ` FOR UPDATE`
	}
	policy := &Policy{OrganizationID: orgID}
	var updatedAt time.Time
	err := tx.QueryRow(ctx, query, orgID).Scan(&policy.ThreadDays, &policy.Revision, &policy.UpdatedBy, &updatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return &Policy{OrganizationID: orgID}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read retention policy: %w", err)
	}
	policy.UpdatedAt = &updatedAt
	return policy, nil
}

// PutPolicy replaces the window if expectedRevision is still current, and records
// the history row and the audit intent in the same transaction.
func (r *Repository) PutPolicy(ctx context.Context, orgID string, actor Actor, expectedRevision int64, threadDays *int) (*Policy, error) {
	if err := r.ready(); err != nil {
		return nil, err
	}
	var saved *Policy
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		before, err := loadPolicy(ctx, tx, orgID, true)
		if err != nil {
			return err
		}
		if before.Revision != expectedRevision {
			return &RevisionConflictError{Current: before.Revision}
		}
		next := before.Revision + 1
		if before.Revision == 0 {
			tag, err := tx.Exec(ctx, `
INSERT INTO org_retention_policies (org_id, thread_days, revision, updated_by)
VALUES ($1, $2, $3, $4)
ON CONFLICT (org_id) DO NOTHING`, orgID, threadDays, next, actor.ID)
			if err != nil {
				return mapWriteError(err)
			}
			if tag.RowsAffected() == 0 {
				// Another first write committed between the read and this insert.
				latest, readErr := loadPolicy(ctx, tx, orgID, true)
				if readErr != nil {
					return readErr
				}
				return &RevisionConflictError{Current: latest.Revision}
			}
		} else if _, err := tx.Exec(ctx, `
UPDATE org_retention_policies
SET thread_days = $2, revision = $3, updated_by = $4, updated_at = NOW()
WHERE org_id = $1`, orgID, threadDays, next, actor.ID); err != nil {
			return mapWriteError(err)
		}

		after, err := loadPolicy(ctx, tx, orgID, false)
		if err != nil {
			return err
		}
		if err := insertHistory(ctx, tx, orgID, "policy", &next, nil, actor.ID,
			map[string]any{"thread_days": before.ThreadDays},
			map[string]any{"thread_days": after.ThreadDays}); err != nil {
			return err
		}
		eventID := fmt.Sprintf("retention:org-core:policy:%s:%d", orgDigest(orgID), next)
		if err := enqueueAudit(ctx, tx, eventID, orgID, actor, "retention_policy:"+orgID, orgID, map[string]any{
			"action": "policy_set", "revision": next,
			"thread_days_before": before.ThreadDays, "thread_days": after.ThreadDays,
		}); err != nil {
			return err
		}
		saved = after
		return nil
	})
	if err != nil {
		return nil, err
	}
	return saved, nil
}

const holdColumns = `id, org_id, scope, target_ref, hold_ref, created_by, created_at, released_by, released_at`

func scanHold(row pgx.Row) (Hold, error) {
	var hold Hold
	err := row.Scan(&hold.ID, &hold.OrganizationID, &hold.Scope, &hold.TargetRef, &hold.HoldRef,
		&hold.CreatedBy, &hold.CreatedAt, &hold.ReleasedBy, &hold.ReleasedAt)
	return hold, err
}

// ListHolds returns every active hold, then the most recent released ones, newest
// first within each, up to MaxListedHolds rows.
func (r *Repository) ListHolds(ctx context.Context, orgID string) ([]Hold, error) {
	if err := r.ready(); err != nil {
		return nil, err
	}
	holds := []Hold{}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `
SELECT `+holdColumns+`
FROM org_legal_holds
WHERE org_id = $1
ORDER BY (released_at IS NULL) DESC, created_at DESC, id
LIMIT $2`, orgID, MaxListedHolds)
		if err != nil {
			return fmt.Errorf("read legal holds: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			hold, err := scanHold(rows)
			if err != nil {
				return fmt.Errorf("scan legal hold: %w", err)
			}
			holds = append(holds, hold)
		}
		return rows.Err()
	})
	if err != nil {
		return nil, err
	}
	return holds, nil
}

// PlaceHold inserts hold unless its target is already held, in which case the
// active hold is returned with Existed set and nothing is written. The partial
// unique index decides a race between two placers.
func (r *Repository) PlaceHold(ctx context.Context, orgID string, actor Actor, hold Hold) (PlaceResult, error) {
	if err := r.ready(); err != nil {
		return PlaceResult{}, err
	}
	var result PlaceResult
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		placed, err := scanHold(tx.QueryRow(ctx, `
INSERT INTO org_legal_holds (id, org_id, scope, target_ref, hold_ref, created_by)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (org_id, scope, target_ref) WHERE released_at IS NULL DO NOTHING
RETURNING `+holdColumns, hold.ID, orgID, hold.Scope, hold.TargetRef, hold.HoldRef, actor.ID))
		if errors.Is(err, pgx.ErrNoRows) {
			existing, readErr := scanHold(tx.QueryRow(ctx, `
SELECT `+holdColumns+`
FROM org_legal_holds
WHERE org_id = $1 AND scope = $2 AND target_ref = $3 AND released_at IS NULL`, orgID, hold.Scope, hold.TargetRef))
			if readErr != nil {
				return fmt.Errorf("read active legal hold: %w", readErr)
			}
			result = PlaceResult{Hold: existing, Existed: true}
			return nil
		}
		if err != nil {
			return mapWriteError(err)
		}
		if err := insertHistory(ctx, tx, orgID, "hold_placed", nil, &placed.ID, actor.ID,
			map[string]any{},
			map[string]any{"scope": placed.Scope, "target_ref": placed.TargetRef, "hold_ref": placed.HoldRef}); err != nil {
			return err
		}
		if err := enqueueAudit(ctx, tx, "retention:org-core:"+placed.ID+":placed", orgID, actor, "legal_hold:"+placed.ID, placed.ID, map[string]any{
			"action": "hold_placed", "hold_id": placed.ID, "scope": placed.Scope, "target_ref": placed.TargetRef,
		}); err != nil {
			return err
		}
		result = PlaceResult{Hold: placed}
		return nil
	})
	if err != nil {
		return PlaceResult{}, err
	}
	return result, nil
}

// ReleaseHold sets the release columns of one active hold. A hold that was already
// released is ErrHoldAlreadyReleased, an unknown one ErrHoldNotFound.
func (r *Repository) ReleaseHold(ctx context.Context, orgID, holdID string, actor Actor) (Hold, error) {
	if err := r.ready(); err != nil {
		return Hold{}, err
	}
	var released Hold
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		hold, err := scanHold(tx.QueryRow(ctx, `
UPDATE org_legal_holds
SET released_by = $3, released_at = NOW()
WHERE org_id = $1 AND id = $2 AND released_at IS NULL
RETURNING `+holdColumns, orgID, holdID, actor.ID))
		if errors.Is(err, pgx.ErrNoRows) {
			var exists bool
			if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM org_legal_holds WHERE org_id = $1 AND id = $2)`, orgID, holdID).Scan(&exists); err != nil {
				return fmt.Errorf("read legal hold: %w", err)
			}
			if exists {
				return ErrHoldAlreadyReleased
			}
			return ErrHoldNotFound
		}
		if err != nil {
			return fmt.Errorf("release legal hold: %w", err)
		}
		if err := insertHistory(ctx, tx, orgID, "hold_released", nil, &hold.ID, actor.ID,
			map[string]any{"released": false},
			map[string]any{"released": true, "scope": hold.Scope, "target_ref": hold.TargetRef}); err != nil {
			return err
		}
		if err := enqueueAudit(ctx, tx, "retention:org-core:"+hold.ID+":released", orgID, actor, "legal_hold:"+hold.ID, hold.ID, map[string]any{
			"action": "hold_released", "hold_id": hold.ID, "scope": hold.Scope, "target_ref": hold.TargetRef,
		}); err != nil {
			return err
		}
		released = hold
		return nil
	})
	if err != nil {
		return Hold{}, err
	}
	return released, nil
}

// Check reads the window, the revision and the active holds that touch threadIDs
// in one transaction, so a hold placed in between cannot be missed against a
// policy read before it. A missing or soft-deleted organisation is
// ErrOrganizationNotFound: the sweeper must find no candidates there, never
// "no hold".
func (r *Repository) Check(ctx context.Context, orgID string, threadIDs []string) (CheckResult, error) {
	if err := r.ready(); err != nil {
		return CheckResult{}, err
	}
	result := CheckResult{OrganizationID: orgID, HeldThreadIDs: []string{}}
	err := r.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var live bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM organizations WHERE id = $1 AND deleted_at IS NULL)`, orgID).Scan(&live); err != nil {
			return fmt.Errorf("read organization: %w", err)
		}
		if !live {
			return ErrOrganizationNotFound
		}
		policy, err := loadPolicy(ctx, tx, orgID, false)
		if err != nil {
			return err
		}
		result.WindowDays = policy.ThreadDays
		result.Revision = policy.Revision
		if threadIDs == nil {
			threadIDs = []string{}
		}
		rows, err := tx.Query(ctx, `
SELECT scope, target_ref
FROM org_legal_holds
WHERE org_id = $1 AND released_at IS NULL
  AND (scope = 'org' OR (scope = 'thread' AND target_ref = ANY($2)))
ORDER BY target_ref`, orgID, threadIDs)
		if err != nil {
			return fmt.Errorf("read active legal holds: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			var scope, target string
			if err := rows.Scan(&scope, &target); err != nil {
				return fmt.Errorf("scan active legal hold: %w", err)
			}
			if scope == ScopeOrg {
				result.OrgHeld = true
				continue
			}
			result.HeldThreadIDs = append(result.HeldThreadIDs, target)
		}
		return rows.Err()
	})
	if err != nil {
		return CheckResult{}, err
	}
	return result, nil
}

func insertHistory(ctx context.Context, tx pgx.Tx, orgID, kind string, revision *int64, holdID *string, actorID string, before, after map[string]any) error {
	beforeJSON, err := json.Marshal(before)
	if err != nil {
		return fmt.Errorf("encode retention history: %w", err)
	}
	afterJSON, err := json.Marshal(after)
	if err != nil {
		return fmt.Errorf("encode retention history: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO org_retention_history (org_id, change_kind, revision, hold_id, actor_id, before_state, after_state)
VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7::jsonb)`, orgID, kind, revision, holdID, actorID, string(beforeJSON), string(afterJSON)); err != nil {
		return fmt.Errorf("record retention history: %w", err)
	}
	return nil
}

// auditPayload is content-free by construction: the window, revision, hold id,
// scope and target id. The hold reference has no path into it (migration 031
// refuses a payload that carries one).
func auditPayload(eventID, orgID string, actor Actor, subject, resourceID string, details map[string]any, occurredAt time.Time) map[string]any {
	return map[string]any{
		"event_id": eventID, "occurred_at": occurredAt.UTC().Format(time.RFC3339Nano),
		"org_id": orgID, "user_id": actor.ID, "actor_role": actor.Role,
		"plane": "control", "producer": "org-core", "event": AuditEvent,
		"subject": subject, "resource_id": resourceID, "outcome": "ok",
		"details": details,
	}
}

func enqueueAudit(ctx context.Context, tx pgx.Tx, eventID, orgID string, actor Actor, subject, resourceID string, details map[string]any) error {
	body, err := json.Marshal(auditPayload(eventID, orgID, actor, subject, resourceID, details, time.Now()))
	if err != nil {
		return fmt.Errorf("encode retention audit payload: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO org_retention_audit_outbox (event_id, org_id, subject, payload)
VALUES ($1, $2, $3, $4)`, eventID, orgID, AuditSubject, body); err != nil {
		return fmt.Errorf("record retention audit outbox: %w", err)
	}
	return nil
}

// orgDigest keeps a policy event id inside the outbox's 128-character bound
// whatever the length of the organisation id.
func orgDigest(orgID string) string {
	sum := sha256.Sum256([]byte(orgID))
	return hex.EncodeToString(sum[:12])
}

// ClaimAudit claims up to limit due rows on the owner pool, as the SIEM outbox does.
func (r *Repository) ClaimAudit(ctx context.Context, limit int) ([]AuditRow, error) {
	if err := r.ready(); err != nil {
		return nil, err
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin retention audit outbox claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id
  FROM org_retention_audit_outbox
  WHERE published_at IS NULL
    AND dead_lettered_at IS NULL
    AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE org_retention_audit_outbox o
SET processing_at = NOW(), updated_at = NOW()
FROM claimed
WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.subject, o.payload`, limit, AuditMaxAttempts)
	if err != nil {
		return nil, fmt.Errorf("claim retention audit outbox: %w", err)
	}
	defer rows.Close()
	claimed := make([]AuditRow, 0)
	for rows.Next() {
		var row AuditRow
		var body []byte
		if err := rows.Scan(&row.EventID, &row.Subject, &body); err != nil {
			return nil, fmt.Errorf("scan retention audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.Payload); err != nil {
			return nil, fmt.Errorf("decode retention audit outbox payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate retention audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit retention audit outbox claim: %w", err)
	}
	return claimed, nil
}

func (r *Repository) MarkAuditPublished(ctx context.Context, eventID string) error {
	if err := r.ready(); err != nil {
		return err
	}
	result, err := r.db.Pool.Exec(ctx, `
UPDATE org_retention_audit_outbox
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1,
    last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, eventID)
	if err != nil {
		return fmt.Errorf("mark retention audit published: %w", err)
	}
	if result.RowsAffected() != 1 {
		return errors.New("retention audit acknowledgement did not match a pending event")
	}
	return nil
}

func (r *Repository) MarkAuditFailed(ctx context.Context, eventID string, publishErr error) (bool, error) {
	if err := r.ready(); err != nil {
		return false, err
	}
	message := "unknown audit publish failure"
	if publishErr != nil {
		message = publishErr.Error()
	}
	if len(message) > 2048 {
		message = message[:2048]
	}
	var deadLettered bool
	err := r.db.Pool.QueryRow(ctx, `
UPDATE org_retention_audit_outbox
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
RETURNING dead_lettered_at IS NOT NULL`, eventID, AuditMaxAttempts, message).Scan(&deadLettered)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return false, errors.New("retention audit failure did not match a pending event")
		}
		return false, fmt.Errorf("record retention audit publish failure: %w", err)
	}
	return deadLettered, nil
}

func mapWriteError(err error) error {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == postgresForeignKeyViolation {
		return ErrOrganizationNotFound
	}
	return fmt.Errorf("write retention record: %w", err)
}
