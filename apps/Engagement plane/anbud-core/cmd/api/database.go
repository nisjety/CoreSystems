package main

import (
	"context"
	"errors"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/db"
)

// The API has no schema-owner credential in a split-role deployment. Its
// migration job is the same image with a different command and secret mount.
type databaseSteps struct {
	open         func(context.Context, string) (*pgxpool.Pool, error)
	close        func(*pgxpool.Pool)
	migrate      func(context.Context, *pgxpool.Pool) error
	runtimeCheck func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error)
	ownerCheck   func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error)
}

func realDatabaseSteps() databaseSteps {
	return databaseSteps{open: db.Open, close: func(p *pgxpool.Pool) { p.Close() }, migrate: db.ApplyMigrations,
		runtimeCheck: db.CheckRuntimeRole, ownerCheck: db.CheckOwnerRole}
}

func openAPIDatabaseWith(ctx context.Context, cfg config.Config, steps databaseSteps) (*pgxpool.Pool, error) {
	dsn := cfg.DatabaseURL.Reveal()
	pool, err := steps.open(ctx, dsn)
	if err != nil {
		return nil, errors.New("could not open API database; check ANBUD_DSN")
	}
	if !cfg.SkipMigrations {
		if err := steps.migrate(ctx, pool); err != nil {
			steps.close(pool)
			return nil, errors.New("could not apply database migrations")
		}
	}
	verdict, err := steps.runtimeCheck(ctx, pool, dsn)
	if err != nil || (cfg.SkipMigrations && verdict != db.RoleOK) {
		steps.close(pool)
		return nil, errors.New("API database runtime role is unavailable or too privileged; use the dedicated anbud_app role")
	}
	return pool, nil
}

func migrateOnlyWith(ctx context.Context, get func(string) string, steps databaseSteps) error {
	dsn := strings.TrimSpace(get(config.EnvMigrateDSN))
	if dsn == "" {
		return errors.New("ANBUD_MIGRATE_DSN is required for --migrate-only")
	}
	pool, err := steps.open(ctx, dsn)
	if err != nil {
		return errors.New("could not open migration database; check ANBUD_MIGRATE_DSN")
	}
	defer steps.close(pool)
	verdict, err := steps.ownerCheck(ctx, pool, dsn)
	if err != nil || verdict != db.RoleOK {
		return errors.New("migration database role is unavailable or too privileged; use the dedicated anbud_migrator role")
	}
	if err := steps.migrate(ctx, pool); err != nil {
		return errors.New("could not apply database migrations")
	}
	return nil
}
