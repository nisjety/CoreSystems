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
	db.webhooks = &webhooksStore{pool: pool}
	db.webhookDeliveries = &webhookDeliveriesStore{pool: pool}
	db.blocklists = &blocklistsStore{pool: pool}
	db.events = &eventLog{pool: pool}
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
	webhooks          *webhooksStore
	webhookDeliveries *webhookDeliveriesStore
	blocklists        *blocklistsStore
	events            *eventLog
}

func (d *postgresDB) Jobs() store.JobsStore                               { return d.jobs }
func (d *postgresDB) Stores() store.ResourceStore[store.NamedStore]       { return d.stores }
func (d *postgresDB) Snapshots() store.ResourceStore[store.Snapshot]      { return d.snaps }
func (d *postgresDB) Artifacts() store.ResourceStore[store.Artifact]      { return d.arts }
func (d *postgresDB) Profiles() store.ResourceStore[store.BrowserProfile] { return d.profiles }
func (d *postgresDB) Schedules() store.SchedulesStore                     { return d.schedules }
func (d *postgresDB) Events() store.EventLog                              { return d.events }

func (d *postgresDB) Webhooks() store.ResourceStore[store.Webhook] { return d.webhooks }
func (d *postgresDB) WebhookDeliveries() store.WebhookDeliveryStore {
	return d.webhookDeliveries
}
func (d *postgresDB) Blocklists() store.ResourceStore[store.BlocklistEntry] { return d.blocklists }

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
