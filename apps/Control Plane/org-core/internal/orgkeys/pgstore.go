package orgkeys

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/jackc/pgx/v5"
)

const (
	kekTable    = "org_key_encryption_keys"
	canaryTable = "org_key_master_canaries"
	outboxTable = "org_key_audit_outbox"
	// auditMaxAttempts matches the other org-core outboxes (and the migration's
	// attempts bound).
	auditMaxAttempts = 8
)

// PostgresStore keeps KEKs in org_key_encryption_keys (migration 030). Org-scoped
// reads and writes run in DB.WithOrgScope, so row-level security holds them to
// the org in the path; the canary table and the outbox publisher use the owner
// pool, like the other org-core outboxes.
type PostgresStore struct {
	db  *database.DB
	now func() time.Time
}

// NewPostgresStore returns the production store.
func NewPostgresStore(db *database.DB) *PostgresStore {
	return &PostgresStore{db: db, now: time.Now}
}

func (p *PostgresStore) ready() error {
	if p == nil || p.db == nil || p.db.Pool == nil {
		return fmt.Errorf("%w: database is not configured", ErrUnavailable)
	}
	return nil
}

func (p *PostgresStore) Canaries(ctx context.Context) (map[string][]byte, error) {
	if err := p.ready(); err != nil {
		return nil, err
	}
	rows, err := p.db.Pool.Query(ctx, `SELECT master_key_id, sealed FROM `+canaryTable)
	if err != nil {
		return nil, fmt.Errorf("read key canaries: %w", err)
	}
	defer rows.Close()
	out := map[string][]byte{}
	for rows.Next() {
		var id string
		var sealed []byte
		if err := rows.Scan(&id, &sealed); err != nil {
			return nil, fmt.Errorf("scan key canary: %w", err)
		}
		out[id] = sealed
	}
	return out, rows.Err()
}

func (p *PostgresStore) PutCanary(ctx context.Context, masterKeyID string, sealed []byte) ([]byte, error) {
	if err := p.ready(); err != nil {
		return nil, err
	}
	if _, err := p.db.Pool.Exec(ctx, `
INSERT INTO `+canaryTable+` (master_key_id, sealed) VALUES ($1, $2)
ON CONFLICT (master_key_id) DO NOTHING`, masterKeyID, sealed); err != nil {
		return nil, fmt.Errorf("store key canary: %w", err)
	}
	var stored []byte
	if err := p.db.Pool.QueryRow(ctx, `SELECT sealed FROM `+canaryTable+` WHERE master_key_id = $1`, masterKeyID).Scan(&stored); err != nil {
		return nil, fmt.Errorf("read key canary: %w", err)
	}
	return stored, nil
}

// lockAndCheck takes the per-org advisory lock (KEK creation and rotation are
// serialised per org) and applies the common preconditions.
func lockAndCheck(ctx context.Context, tx pgx.Tx, orgID string, lock bool) error {
	if lock {
		if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended('org-core.orgkeys:' || $1, 0))`, orgID); err != nil {
			return fmt.Errorf("lock org keys: %w", err)
		}
	}
	var exists, destroyed bool
	if err := tx.QueryRow(ctx, `
SELECT EXISTS(SELECT 1 FROM organizations WHERE id = $1),
       EXISTS(SELECT 1 FROM `+kekTable+` WHERE org_id = $1 AND state = 'destroyed')`, orgID).Scan(&exists, &destroyed); err != nil {
		return fmt.Errorf("check org keys: %w", err)
	}
	if !exists {
		return ErrOrgNotFound
	}
	if destroyed {
		return ErrDestroyed
	}
	return nil
}

const kekColumns = `org_id, kek_version, master_key_id, wrapped_kek, state, created_at, retired_at, destroyed_at`

func scanKEK(row pgx.Row) (KEKRecord, error) {
	var rec KEKRecord
	var masterID *string
	err := row.Scan(&rec.OrgID, &rec.Version, &masterID, &rec.Wrapped, &rec.State, &rec.CreatedAt, &rec.RetiredAt, &rec.DestroyedAt)
	if masterID != nil {
		rec.MasterKeyID = *masterID
	}
	return rec, err
}

func insertKEK(ctx context.Context, tx pgx.Tx, orgID string, build BuildKEK) (KEKRecord, error) {
	var version int
	if err := tx.QueryRow(ctx, `SELECT COALESCE(MAX(kek_version), 0) + 1 FROM `+kekTable+` WHERE org_id = $1`, orgID).Scan(&version); err != nil {
		return KEKRecord{}, fmt.Errorf("next kek version: %w", err)
	}
	masterID, wrapped, err := build(version)
	if err != nil {
		return KEKRecord{}, err
	}
	return scanKEK(tx.QueryRow(ctx, `
INSERT INTO `+kekTable+` (org_id, kek_version, master_key_id, wrapped_kek, state)
VALUES ($1, $2, $3, $4, 'active')
RETURNING `+kekColumns, orgID, version, masterID, wrapped))
}

func enqueueAudit(ctx context.Context, tx pgx.Tx, event AuditEvent, now time.Time) error {
	eventID := newAuditEventID()
	body, err := json.Marshal(auditPayload(eventID, event, now))
	if err != nil {
		return fmt.Errorf("encode org key audit payload: %w", err)
	}
	if _, err := tx.Exec(ctx, `
INSERT INTO `+outboxTable+` (event_id, org_id, subject, payload) VALUES ($1, $2, $3, $4)`,
		eventID, event.OrgID, AuditSubject, body); err != nil {
		return fmt.Errorf("record org key audit outbox: %w", err)
	}
	return nil
}

func (p *PostgresStore) EnsureActiveKEK(ctx context.Context, orgID string, build BuildKEK, audit AuditEvent) (KEKRecord, bool, error) {
	if err := p.ready(); err != nil {
		return KEKRecord{}, false, err
	}
	// Fast path without the lock: the active KEK usually exists.
	var rec KEKRecord
	var created bool
	err := p.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockAndCheck(ctx, tx, orgID, false); err != nil {
			return err
		}
		found, err := scanKEK(tx.QueryRow(ctx, `SELECT `+kekColumns+` FROM `+kekTable+` WHERE org_id = $1 AND state = 'active'`, orgID))
		if err == nil {
			rec = found
			return nil
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("read active kek: %w", err)
		}
		return pgx.ErrNoRows
	})
	if err == nil {
		return rec, false, nil
	}
	if !errors.Is(err, pgx.ErrNoRows) {
		return KEKRecord{}, false, err
	}
	err = p.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockAndCheck(ctx, tx, orgID, true); err != nil {
			return err
		}
		found, err := scanKEK(tx.QueryRow(ctx, `SELECT `+kekColumns+` FROM `+kekTable+` WHERE org_id = $1 AND state = 'active'`, orgID))
		if err == nil {
			rec = found
			return nil
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return fmt.Errorf("read active kek: %w", err)
		}
		inserted, err := insertKEK(ctx, tx, orgID, build)
		if err != nil {
			return err
		}
		audit.KEKVersion = inserted.Version
		if err := enqueueAudit(ctx, tx, audit, p.now()); err != nil {
			return err
		}
		rec, created = inserted, true
		return nil
	})
	if err != nil {
		return KEKRecord{}, false, err
	}
	return rec, created, nil
}

func (p *PostgresStore) RotateKEK(ctx context.Context, orgID string, build BuildKEK, audit AuditEvent) (KEKRecord, error) {
	if err := p.ready(); err != nil {
		return KEKRecord{}, err
	}
	var rec KEKRecord
	err := p.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockAndCheck(ctx, tx, orgID, true); err != nil {
			return err
		}
		// Retire first: the partial unique index allows one active row per org.
		if _, err := tx.Exec(ctx, `
UPDATE `+kekTable+` SET state = 'retired', retired_at = NOW()
WHERE org_id = $1 AND state = 'active'`, orgID); err != nil {
			return fmt.Errorf("retire kek: %w", err)
		}
		inserted, err := insertKEK(ctx, tx, orgID, build)
		if err != nil {
			return err
		}
		audit.KEKVersion = inserted.Version
		if err := enqueueAudit(ctx, tx, audit, p.now()); err != nil {
			return err
		}
		rec = inserted
		return nil
	})
	return rec, err
}

func (p *PostgresStore) GetKEK(ctx context.Context, orgID string, version int) (KEKRecord, int, error) {
	if err := p.ready(); err != nil {
		return KEKRecord{}, 0, err
	}
	var rec KEKRecord
	var active int
	err := p.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockAndCheck(ctx, tx, orgID, false); err != nil {
			return err
		}
		if err := tx.QueryRow(ctx, `SELECT COALESCE(MAX(kek_version) FILTER (WHERE state = 'active'), 0) FROM `+kekTable+` WHERE org_id = $1`, orgID).Scan(&active); err != nil {
			return fmt.Errorf("read active kek version: %w", err)
		}
		found, err := scanKEK(tx.QueryRow(ctx, `SELECT `+kekColumns+` FROM `+kekTable+` WHERE org_id = $1 AND kek_version = $2`, orgID, version))
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNoKey
		}
		if err != nil {
			return fmt.Errorf("read kek: %w", err)
		}
		rec = found
		return nil
	})
	return rec, active, err
}

func (p *PostgresStore) ListKEKs(ctx context.Context, orgID string) ([]KEKRecord, error) {
	if err := p.ready(); err != nil {
		return nil, err
	}
	var out []KEKRecord
	err := p.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		var exists bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS(SELECT 1 FROM organizations WHERE id = $1)`, orgID).Scan(&exists); err != nil {
			return fmt.Errorf("check org: %w", err)
		}
		if !exists {
			return ErrOrgNotFound
		}
		rows, err := tx.Query(ctx, `SELECT `+kekColumns+` FROM `+kekTable+` WHERE org_id = $1 ORDER BY kek_version`, orgID)
		if err != nil {
			return fmt.Errorf("list keks: %w", err)
		}
		defer rows.Close()
		for rows.Next() {
			rec, err := scanKEK(rows)
			if err != nil {
				return fmt.Errorf("scan kek: %w", err)
			}
			out = append(out, rec)
		}
		return rows.Err()
	})
	return out, err
}

func (p *PostgresStore) DestroyKEKs(ctx context.Context, orgID string, audit AuditEvent) (int, error) {
	if err := p.ready(); err != nil {
		return 0, err
	}
	var destroyed int
	err := p.db.WithOrgScope(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockAndCheck(ctx, tx, orgID, true); err != nil {
			return err
		}
		tag, err := tx.Exec(ctx, `
UPDATE `+kekTable+`
SET wrapped_kek = NULL, master_key_id = NULL, state = 'destroyed', destroyed_at = NOW()
WHERE org_id = $1`, orgID)
		if err != nil {
			return fmt.Errorf("destroy keks: %w", err)
		}
		destroyed = int(tag.RowsAffected())
		if destroyed == 0 {
			// The tombstone keeps the org shredded: a later data-key call answers
			// 410 instead of quietly minting a fresh KEK after erasure.
			if _, err := tx.Exec(ctx, `
INSERT INTO `+kekTable+` (org_id, kek_version, state, destroyed_at) VALUES ($1, 0, 'destroyed', NOW())`, orgID); err != nil {
				return fmt.Errorf("record key tombstone: %w", err)
			}
		}
		audit.Count = destroyed
		if err := enqueueAudit(ctx, tx, audit, p.now()); err != nil {
			return err
		}
		return nil
	})
	return destroyed, err
}

func (p *PostgresStore) RecordAudit(ctx context.Context, audit AuditEvent) error {
	if err := p.ready(); err != nil {
		return err
	}
	return p.db.WithOrgScope(ctx, audit.OrgID, func(tx pgx.Tx) error {
		if err := enqueueAudit(ctx, tx, audit, p.now()); err != nil {
			return err
		}
		return nil
	})
}

// FlushAudit claims, publishes and marks outbox rows on the owner pool, the loop
// FlushWebPolicyAuditOutbox runs.
func (p *PostgresStore) FlushAudit(ctx context.Context, publish PublishFunc, limit int) (int, error) {
	if err := p.ready(); err != nil {
		return 0, err
	}
	if limit < 1 || limit > 1000 {
		return 0, fmt.Errorf("org key audit outbox limit must be between 1 and 1000")
	}
	type claimedRow struct {
		eventID string
		subject string
		payload map[string]any
	}
	tx, err := p.db.Pool.Begin(ctx)
	if err != nil {
		return 0, fmt.Errorf("begin org key audit claim: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	rows, err := tx.Query(ctx, `
WITH claimed AS (
  SELECT event_id FROM `+outboxTable+`
  WHERE published_at IS NULL AND dead_lettered_at IS NULL AND attempts < $2
    AND next_attempt_at <= NOW()
    AND (processing_at IS NULL OR processing_at < NOW() - INTERVAL '1 minute')
  ORDER BY next_attempt_at, created_at, event_id
  LIMIT $1
  FOR UPDATE SKIP LOCKED
)
UPDATE `+outboxTable+` o SET processing_at = NOW(), updated_at = NOW()
FROM claimed WHERE o.event_id = claimed.event_id
RETURNING o.event_id, o.subject, o.payload`, limit, auditMaxAttempts)
	if err != nil {
		return 0, fmt.Errorf("claim org key audit outbox: %w", err)
	}
	claimed := make([]claimedRow, 0)
	for rows.Next() {
		var row claimedRow
		var body []byte
		if err := rows.Scan(&row.eventID, &row.subject, &body); err != nil {
			rows.Close()
			return 0, fmt.Errorf("scan org key audit outbox: %w", err)
		}
		if err := json.Unmarshal(body, &row.payload); err != nil {
			rows.Close()
			return 0, fmt.Errorf("decode org key audit payload: %w", err)
		}
		claimed = append(claimed, row)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return 0, fmt.Errorf("iterate org key audit outbox: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return 0, fmt.Errorf("commit org key audit claim: %w", err)
	}

	published := 0
	var errs error
	for _, row := range claimed {
		if publishErr := publish(ctx, row.subject, row.eventID, row.payload); publishErr != nil {
			message := publishErr.Error()
			if len(message) > 2048 {
				message = message[:2048]
			}
			_, markErr := p.db.Pool.Exec(ctx, `
UPDATE `+outboxTable+`
SET attempts = attempts + 1, processing_at = NULL,
    next_attempt_at = NOW() + LEAST(INTERVAL '5 minutes', INTERVAL '1 second' * power(2, LEAST(attempts, 8))),
    dead_lettered_at = CASE WHEN attempts + 1 >= $2 THEN NOW() ELSE NULL END,
    last_error = $3, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, row.eventID, auditMaxAttempts, message)
			errs = errors.Join(errs, publishErr, markErr)
			continue
		}
		if _, err := p.db.Pool.Exec(ctx, `
UPDATE `+outboxTable+`
SET published_at = NOW(), processing_at = NULL, attempts = attempts + 1, last_error = NULL, updated_at = NOW()
WHERE event_id = $1 AND published_at IS NULL AND dead_lettered_at IS NULL`, row.eventID); err != nil {
			errs = errors.Join(errs, fmt.Errorf("mark org key audit published: %w", err))
			continue
		}
		published++
	}
	return published, errs
}
