package siem

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

// Cursor is a destination's delivery position. Seq is the highest chain_seq a
// receiver has acknowledged with a 2xx; zero means nothing has been delivered.
type Cursor struct {
	Seq                 int64
	ConsecutiveFailures int
}

// Cursors persists delivery positions, so a restart resumes instead of
// resending an org's whole chain.
type Cursors interface {
	Get(ctx context.Context, destinationID string) (Cursor, error)
	// Advance records a 2xx: the cursor moves to seq (never backwards), the
	// failure streak resets and delivered rows are added to the total.
	Advance(ctx context.Context, d Destination, seq int64, delivered int) error
	// Fail records a failed attempt with its fixed-vocabulary code and returns
	// the new failure streak. The cursor does not move.
	Fail(ctx context.Context, d Destination, code string) (int, error)
}

// Source reads the chain the exporter delivers.
type Source interface {
	// Page returns up to limit chained rows of the org after afterSeq, in
	// chain_seq order, with the org's chain head.
	Page(ctx context.Context, orgID string, afterSeq int64, limit int) (store.CompliancePage, error)
	// NewestAttestation is the org's newest signed head, or nil when none.
	NewestAttestation(ctx context.Context, orgID string) (*verifier.Attestation, error)
}

// PGCursors implements Cursors on audit_export_cursors.
type PGCursors struct{ pool *pgxpool.Pool }

func NewPGCursors(pool *pgxpool.Pool) *PGCursors { return &PGCursors{pool: pool} }

// WaitReady blocks until the audit-core server has migrated the cursor table.
// The exporter never migrates: the server owns the schema, and two migrators
// racing on one ledger is the failure this avoids.
func (c *PGCursors) WaitReady(ctx context.Context, every time.Duration, onWait func()) error {
	for {
		var present bool
		err := c.pool.QueryRow(ctx, `SELECT to_regclass('audit_export_cursors') IS NOT NULL`).Scan(&present)
		if err == nil && present {
			return nil
		}
		if onWait != nil {
			onWait()
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(every):
		}
	}
}

func (c *PGCursors) Get(ctx context.Context, destinationID string) (Cursor, error) {
	var cur Cursor
	err := c.pool.QueryRow(ctx,
		`SELECT cursor_seq, consecutive_failures FROM audit_export_cursors WHERE destination_id = $1`,
		destinationID,
	).Scan(&cur.Seq, &cur.ConsecutiveFailures)
	if errors.Is(err, pgx.ErrNoRows) {
		return Cursor{}, nil
	}
	if err != nil {
		return Cursor{}, fmt.Errorf("read export cursor: %w", err)
	}
	return cur, nil
}

func (c *PGCursors) Advance(ctx context.Context, d Destination, seq int64, delivered int) error {
	_, err := c.pool.Exec(ctx, `
		INSERT INTO audit_export_cursors
		  (destination_id, org_id, kind, cursor_seq, delivered_total, last_attempt_at, last_success_at)
		VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
		ON CONFLICT (destination_id) DO UPDATE SET
		  org_id = EXCLUDED.org_id,
		  kind = EXCLUDED.kind,
		  cursor_seq = GREATEST(audit_export_cursors.cursor_seq, EXCLUDED.cursor_seq),
		  delivered_total = audit_export_cursors.delivered_total + EXCLUDED.delivered_total,
		  consecutive_failures = 0,
		  last_error = '',
		  last_attempt_at = NOW(),
		  last_success_at = NOW(),
		  updated_at = NOW()
	`, d.ID, d.OrgID, d.Kind, seq, delivered)
	if err != nil {
		return fmt.Errorf("advance export cursor: %w", err)
	}
	return nil
}

func (c *PGCursors) Fail(ctx context.Context, d Destination, code string) (int, error) {
	var streak int
	err := c.pool.QueryRow(ctx, `
		INSERT INTO audit_export_cursors
		  (destination_id, org_id, kind, consecutive_failures, last_error, last_attempt_at)
		VALUES ($1, $2, $3, 1, $4, NOW())
		ON CONFLICT (destination_id) DO UPDATE SET
		  org_id = EXCLUDED.org_id,
		  kind = EXCLUDED.kind,
		  consecutive_failures = audit_export_cursors.consecutive_failures + 1,
		  last_error = EXCLUDED.last_error,
		  last_attempt_at = NOW(),
		  updated_at = NOW()
		RETURNING consecutive_failures
	`, d.ID, d.OrgID, d.Kind, code).Scan(&streak)
	if err != nil {
		return 0, fmt.Errorf("record export failure: %w", err)
	}
	return streak, nil
}

// PGSource implements Source on the audit tables. It reads through the store's
// compliance read, so the rows it exports carry exactly the fields the chain
// hash covers and verify offline like any other export.
type PGSource struct {
	pool  *pgxpool.Pool
	store *store.Store
}

func NewPGSource(pool *pgxpool.Pool) *PGSource {
	return &PGSource{pool: pool, store: store.New(pool)}
}

func (s *PGSource) Page(ctx context.Context, orgID string, afterSeq int64, limit int) (store.CompliancePage, error) {
	return s.store.ComplianceEvents(ctx, store.ComplianceFilter{OrgID: orgID, AfterSeq: afterSeq, Limit: limit})
}

func (s *PGSource) NewestAttestation(ctx context.Context, orgID string) (*verifier.Attestation, error) {
	var att verifier.Attestation
	var hash, signature []byte
	err := s.pool.QueryRow(ctx, `
		SELECT chain_seq, row_hash, key_id, signature, signed_at
		FROM audit_chain_attestations
		WHERE org_id = $1
		ORDER BY chain_seq DESC
		LIMIT 1
	`, orgID).Scan(&att.Seq, &hash, &att.KeyID, &signature, &att.SignedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read newest attestation: %w", err)
	}
	att.Hash, att.Signature, att.SignedAt = hash, signature, att.SignedAt.UTC()
	return &att, nil
}
