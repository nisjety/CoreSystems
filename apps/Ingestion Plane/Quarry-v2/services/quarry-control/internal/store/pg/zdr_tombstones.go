// Postgres backend for the ZDR run tombstones (WS4.J, migration 015). The
// in-memory twin and the allowlist are in internal/store/zdr_tombstones.go.
//
// The writer is `INSERT ... ON CONFLICT (run_id) DO NOTHING`: first write wins, a
// retry is a no-op and never reaches the immutability trigger. Every value is
// validated by store.ZdrTombstone.Validate before the statement runs, and the
// table's CHECK constraints refuse anything outside the vocabulary again.
package pg

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

type zdrTombstonesStore struct{ pool *pgxpool.Pool }

func (s *zdrTombstonesStore) Put(t store.ZdrTombstone) (bool, error) {
	if err := t.Validate(); err != nil {
		return false, err
	}
	tag, err := s.pool.Exec(context.Background(), `
		INSERT INTO zdr_run_tombstones
		  (run_id, org_id, kind, retention_class, outcome, driver, started_at, ended_at, step_count, cost_units, error_class)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
		ON CONFLICT (run_id) DO NOTHING`,
		t.RunID, t.OrgID, t.Kind, t.RetentionClass, t.Outcome, t.Driver,
		t.StartedAt, t.EndedAt.UTC(), t.StepCount, t.CostUnits, t.ErrorClass)
	if err != nil {
		return false, fmt.Errorf("pg: zdr tombstone insert: %w", err)
	}
	return tag.RowsAffected() == 1, nil
}

func (s *zdrTombstonesStore) DeleteBefore(cutoff time.Time, limit int) (int64, error) {
	if limit <= 0 {
		return 0, nil
	}
	tag, err := s.pool.Exec(context.Background(), `
		DELETE FROM zdr_run_tombstones
		WHERE run_id IN (
		  SELECT run_id FROM zdr_run_tombstones
		  WHERE ended_at < $1
		  ORDER BY ended_at
		  LIMIT $2)`, cutoff.UTC(), limit)
	if err != nil {
		return 0, fmt.Errorf("pg: zdr tombstone delete: %w", err)
	}
	return tag.RowsAffected(), nil
}

func (s *zdrTombstonesStore) CountBefore(cutoff time.Time) (int64, error) {
	var n int64
	if err := s.pool.QueryRow(context.Background(),
		`SELECT count(*) FROM zdr_run_tombstones WHERE ended_at < $1`, cutoff.UTC()).Scan(&n); err != nil {
		return 0, fmt.Errorf("pg: zdr tombstone count: %w", err)
	}
	return n, nil
}

func (s *zdrTombstonesStore) ListByOrg(orgID string, limit int) ([]store.ZdrTombstone, error) {
	if strings.TrimSpace(orgID) == "" || limit <= 0 {
		return nil, nil
	}
	rows, err := s.pool.Query(context.Background(), `
		SELECT run_id, org_id, kind, retention_class, outcome, driver, started_at, ended_at,
		       step_count, cost_units::float8, error_class, created_at
		FROM zdr_run_tombstones
		WHERE org_id = $1
		ORDER BY ended_at DESC, run_id DESC
		LIMIT $2`, orgID, limit)
	if err != nil {
		return nil, fmt.Errorf("pg: zdr tombstone list: %w", err)
	}
	defer rows.Close()
	var out []store.ZdrTombstone
	for rows.Next() {
		var t store.ZdrTombstone
		var step *int32
		if err := rows.Scan(&t.RunID, &t.OrgID, &t.Kind, &t.RetentionClass, &t.Outcome, &t.Driver,
			&t.StartedAt, &t.EndedAt, &step, &t.CostUnits, &t.ErrorClass, &t.CreatedAt); err != nil {
			return nil, err
		}
		if step != nil {
			v := int(*step)
			t.StepCount = &v
		}
		out = append(out, t)
	}
	return out, rows.Err()
}
