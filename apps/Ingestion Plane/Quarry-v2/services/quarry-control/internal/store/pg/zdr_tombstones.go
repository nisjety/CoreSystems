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
	"errors"
	"fmt"
	"strings"
	"sync/atomic"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// The bounds on a tombstone Put. A Put waits on the org's advisory lock while an
// erasure of that org holds it (PurgeOrg is one large transaction). Without bounds,
// a burst of Puts for the erased org (a sweeper expiring many agent runs at once)
// would each hold one of the shared pool's 16 connections until the purge commits
// and starve the rest of quarry-control. So: at most zdrPutSlots Puts run at once
// (the rest wait for a slot without a connection), a Put gives up on the lock after
// zdrPutLockTimeout, and the whole Put is bounded by zdrPutTimeout.
const (
	zdrPutSlots       = 4
	zdrPutLockTimeout = 2 * time.Second
	zdrPutTimeout     = 5 * time.Second
)

// ErrZdrTombstoneBusy is returned by Put when it could not get a slot or the org
// lock in time. The receiver answers 500 and the edge counts the delivery as failed
// (emission is fail-open); nothing is stored.
var ErrZdrTombstoneBusy = errors.New("pg: zdr tombstone busy (org erasure in progress or too many writers)")

type zdrTombstonesStore struct {
	pool *pgxpool.Pool
	// markers is the SetErasureMarkers switch read by PurgeOrg; false writes none.
	markers atomic.Bool
	slots   chan struct{}
	// lockTimeout and putTimeout are fields so a test can shorten them.
	lockTimeout time.Duration
	putTimeout  time.Duration
}

func newZdrTombstonesStore(pool *pgxpool.Pool) *zdrTombstonesStore {
	return &zdrTombstonesStore{
		pool:        pool,
		slots:       make(chan struct{}, zdrPutSlots),
		lockTimeout: zdrPutLockTimeout,
		putTimeout:  zdrPutTimeout,
	}
}

func (s *zdrTombstonesStore) SetErasureMarkers(on bool) { s.markers.Store(on) }

// orgLockSQL serialises a tombstone Put with the org's erasure (PurgeOrg takes the
// same transaction-scoped advisory lock). Without it a Put could read "no marker"
// just before the erasure commits and insert its row just after the erasure's DELETE.
const orgLockSQL = `SELECT pg_advisory_xact_lock(hashtext('zdr_tombstone_org:' || $1::text))`

func (s *zdrTombstonesStore) Put(t store.ZdrTombstone) (bool, error) {
	if err := t.Validate(); err != nil {
		return false, err
	}
	ctx, cancel := context.WithTimeout(context.Background(), s.putTimeout)
	defer cancel()
	// A slot first, so a burst waits here without holding a pool connection.
	select {
	case s.slots <- struct{}{}:
		defer func() { <-s.slots }()
	case <-ctx.Done():
		return false, ErrZdrTombstoneBusy
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		if ctx.Err() != nil {
			return false, ErrZdrTombstoneBusy
		}
		return false, fmt.Errorf("pg: zdr tombstone begin: %w", err)
	}
	defer func() { _ = tx.Rollback(context.Background()) }() // no-op once Commit succeeds
	// Bound the wait on the org lock (and on any row lock) inside this transaction.
	if _, err := tx.Exec(ctx, `SELECT set_config('lock_timeout', $1, true)`,
		fmt.Sprintf("%dms", s.lockTimeout.Milliseconds())); err != nil {
		return false, fmt.Errorf("pg: zdr tombstone lock timeout: %w", err)
	}
	if _, err := tx.Exec(ctx, orgLockSQL, t.OrgID); err != nil {
		if isLockWaitGivenUp(err) || ctx.Err() != nil {
			return false, ErrZdrTombstoneBusy
		}
		return false, fmt.Errorf("pg: zdr tombstone org lock: %w", err)
	}
	// An erased org (a marker row) gets no new tombstone; the answer is the same
	// no-op as a duplicate run id.
	tag, err := tx.Exec(ctx, `
		INSERT INTO zdr_run_tombstones
		  (run_id, org_id, kind, retention_class, outcome, driver, started_at, ended_at, step_count, cost_units, error_class)
		SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11
		WHERE NOT EXISTS (SELECT 1 FROM zdr_tombstone_erasures WHERE org_id = $2)
		ON CONFLICT (run_id) DO NOTHING`,
		t.RunID, t.OrgID, t.Kind, t.RetentionClass, t.Outcome, t.Driver,
		t.StartedAt, t.EndedAt.UTC(), t.StepCount, t.CostUnits, t.ErrorClass)
	if err != nil {
		return false, fmt.Errorf("pg: zdr tombstone insert: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return false, fmt.Errorf("pg: zdr tombstone commit: %w", err)
	}
	return tag.RowsAffected() == 1, nil
}

// isLockWaitGivenUp reports SQLSTATE 55P03 (lock_not_available), what lock_timeout raises.
func isLockWaitGivenUp(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == "55P03"
}

func (s *zdrTombstonesStore) DeleteErasureMarkersBefore(cutoff time.Time) (int64, error) {
	tag, err := s.pool.Exec(context.Background(),
		`DELETE FROM zdr_tombstone_erasures WHERE erased_at < $1`, cutoff.UTC())
	if err != nil {
		return 0, fmt.Errorf("pg: zdr tombstone erasure marker delete: %w", err)
	}
	return tag.RowsAffected(), nil
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
