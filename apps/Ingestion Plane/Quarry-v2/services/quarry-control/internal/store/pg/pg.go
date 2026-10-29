// Package pg — Postgres backend for the control-plane DB interface.
// Identical semantics to store.NewMemory(). Phase 1.1 requirement.
package pg

import (
	"context"
	"embed"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

//go:embed migrations/*.sql
var migrationFS embed.FS

// New opens a pgxpool and applies pending migrations.
func New(ctx context.Context, dsn string) (store.DB, error) {
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		return nil, fmt.Errorf("parse dsn: %w", err)
	}
	cfg.MaxConns = 16
	cfg.MinConns = 1
	cfg.MaxConnLifetime = 30 * time.Minute

	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		return nil, fmt.Errorf("pool: %w", err)
	}
	pingCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := pool.Ping(pingCtx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("ping: %w", err)
	}
	if err := migrate(ctx, pool); err != nil {
		pool.Close()
		return nil, fmt.Errorf("migrate: %w", err)
	}
	db := &postgresDB{pool: pool}
	db.jobs = &jobsStore{pool: pool}
	db.stores = &storesStore{pool: pool}
	db.snaps = &snapshotsStore{pool: pool}
	db.arts = &artifactsStore{pool: pool}
	db.profiles = &profilesStore{pool: pool}
	db.schedules = &schedulesStore{pool: pool}
	db.sources = &sourcesStore{pool: pool}
	db.webhooks = &webhooksStore{pool: pool}
	db.webhookDeliveries = &webhookDeliveriesStore{pool: pool}
	db.blocklists = &blocklistsStore{pool: pool}
	db.events = &eventLog{pool: pool}
	db.teamUsage = &teamUsageStore{pool: pool}
	db.snapshotsV2 = &snapshotsV2Store{pool: pool}
	return db, nil
}

type postgresDB struct {
	pool              *pgxpool.Pool
	jobs              *jobsStore
	stores            *storesStore
	snaps             *snapshotsStore
	arts              *artifactsStore
	profiles          *profilesStore
	schedules         *schedulesStore
	sources           *sourcesStore
	webhooks          *webhooksStore
	webhookDeliveries *webhookDeliveriesStore
	blocklists        *blocklistsStore
	events            *eventLog
	teamUsage         *teamUsageStore
	snapshotsV2       *snapshotsV2Store
}

func (d *postgresDB) Jobs() store.JobsStore                               { return d.jobs }
func (d *postgresDB) Stores() store.ResourceStore[store.NamedStore]       { return d.stores }
func (d *postgresDB) Snapshots() store.ResourceStore[store.Snapshot]      { return d.snaps }
func (d *postgresDB) Artifacts() store.ResourceStore[store.Artifact]      { return d.arts }
func (d *postgresDB) Profiles() store.ResourceStore[store.BrowserProfile] { return d.profiles }
func (d *postgresDB) Schedules() store.SchedulesStore                     { return d.schedules }
func (d *postgresDB) Sources() store.SourcesStore                         { return d.sources }
func (d *postgresDB) Events() store.EventLog                              { return d.events }

func (d *postgresDB) Webhooks() store.ResourceStore[store.Webhook] { return d.webhooks }
func (d *postgresDB) WebhookDeliveries() store.WebhookDeliveryStore {
	return d.webhookDeliveries
}
func (d *postgresDB) Blocklists() store.ResourceStore[store.BlocklistEntry] { return d.blocklists }
func (d *postgresDB) TeamUsage() store.TeamUsageStore                     { return d.teamUsage }
func (d *postgresDB) SnapshotsV2() store.SnapshotsV2Store                 { return d.snapshotsV2 }

// ListRequestQueues is a read-only projection over Quarry runtime's queue
// tables. The control plane deliberately does not own these migrations or
// write paths; it only exposes tenant-scoped operational visibility.
func (d *postgresDB) ListRequestQueues(orgID string, limit int, cur string) ([]store.RequestQueueSummary, string, error) {
	limit = pageLimit(limit, defaultMaxPage)
	c, err := decodeCursor(cur)
	if err != nil {
		return nil, "", err
	}
	q := `SELECT q.queue_id::text, q.name,
	              COUNT(i.request_id) FILTER (WHERE i.status = 'queued'),
	              COUNT(i.request_id) FILTER (WHERE i.status = 'in_flight'),
	              (EXTRACT(EPOCH FROM q.created_at) * 1000)::bigint
	       FROM quarry_request_queues q
	       LEFT JOIN quarry_queue_items i
	         ON i.queue_id = q.queue_id AND i.org_id = q.org_id
	       WHERE q.org_id = $1 AND q.deleted_at IS NULL`
	args := []any{orgID}
	if c != nil {
		q += ` AND (q.created_at, q.queue_id::text) < (to_timestamp($2 / 1000.0), $3)`
		args = append(args, c.CreatedAt, c.ID)
	}
	q += ` GROUP BY q.queue_id, q.name, q.created_at
	       ORDER BY q.created_at DESC, q.queue_id::text DESC
	       LIMIT $` + fmt.Sprint(len(args)+1)
	args = append(args, limit+1)

	rows, err := d.pool.Query(context.Background(), q, args...)
	if err != nil {
		return nil, "", err
	}
	defer rows.Close()
	out := make([]store.RequestQueueSummary, 0, limit)
	for rows.Next() {
		var item store.RequestQueueSummary
		if err := rows.Scan(&item.QueueID, &item.Name, &item.Queued, &item.InFlight, &item.CreatedAt); err != nil {
			return nil, "", err
		}
		out = append(out, item)
	}
	if err := rows.Err(); err != nil {
		return nil, "", err
	}
	var next string
	if len(out) > limit {
		last := out[limit-1]
		next = encodeCursor(last.CreatedAt, last.QueueID)
		out = out[:limit]
	}
	return out, next, nil
}

// Close is callable by operators via type assertion; not part of DB.
func (d *postgresDB) Close() { d.pool.Close() }

// Ping exposes the underlying pgxpool ping for the /ready handler so
// the load balancer can drop a control instance whose DB pool is dead.
func (d *postgresDB) Ping(ctx context.Context) error { return d.pool.Ping(ctx) }

// ---- migrator -------------------------------------------------------------

func migrate(ctx context.Context, pool *pgxpool.Pool) error {
	if _, err := pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS schema_migrations (
			name       TEXT PRIMARY KEY,
			applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
		);`); err != nil {
		return err
	}
	entries, err := migrationFS.ReadDir("migrations")
	if err != nil {
		return err
	}
	names := make([]string, 0, len(entries))
	for _, e := range entries {
		if !strings.HasSuffix(e.Name(), ".sql") {
			continue
		}
		names = append(names, e.Name())
	}
	sort.Strings(names)

	for _, name := range names {
		var exists bool
		if err := pool.QueryRow(ctx,
			`SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE name=$1)`, name,
		).Scan(&exists); err != nil {
			return err
		}
		if exists {
			continue
		}
		sql, err := migrationFS.ReadFile("migrations/" + name)
		if err != nil {
			return err
		}
		tx, err := pool.Begin(ctx)
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, string(sql)); err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("apply %s: %w", name, err)
		}
		if _, err := tx.Exec(ctx,
			`INSERT INTO schema_migrations(name) VALUES ($1)`, name,
		); err != nil {
			_ = tx.Rollback(ctx)
			return err
		}
		if err := tx.Commit(ctx); err != nil {
			return err
		}
	}
	return nil
}

// ---- shared helpers -------------------------------------------------------

func mapPgErr(err error) error {
	if err == nil {
		return nil
	}
	if errors.Is(err, pgx.ErrNoRows) {
		return store.ErrNotFound
	}
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == "23505" { // unique_violation
		return store.ErrConflict
	}
	return err
}

func pageLimit(limit int, max int) int {
	if limit <= 0 || limit > max {
		return max
	}
	return limit
}
