package main

import (
	"context"
	"errors"
	"fmt"
	"io"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/db"
)

// The role model (docs/ROLES.md): migrations run as the schema's owner, through
// ANBUD_MIGRATE_DSN, and the engine writes through ANBUD_DSN, a role that owns
// nothing and so cannot drop or disable the append-only triggers. This file is the
// part of cli that opens the pool the engine writes through, applies the migrations
// through whichever DSN is meant to, and refuses a runtime role that could undo the
// triggers, and an owner role that is a superuser or can make roles or databases.
// Every message here is fixed text that names a variable and never a value: pgx errors
// can quote the DSN, so none of their text is printed. The variable's name is
// config.EnvMigrateDSN, declared once, in internal/config.

// storeOps are the steps openStore takes, so a test can stand in for each. The
// real ones are the pool opener, the two role checks and the migration runner.
type storeOps struct {
	open       func(ctx context.Context, dsn string) (*pgxpool.Pool, error)
	checkOwner func(ctx context.Context, q db.RowQuerier, dsn string) (db.RoleVerdict, error)
	migrate    func(ctx context.Context, pool *pgxpool.Pool) error
	checkRole  func(ctx context.Context, q db.RowQuerier, dsn string) (db.RoleVerdict, error)
}

func realStoreOps() storeOps {
	return storeOps{open: db.Open, checkOwner: db.CheckOwnerRole, migrate: db.ApplyMigrations, checkRole: db.CheckRuntimeRole}
}

// storeSteps are the steps cli uses. Tests replace them for one test at a time.
//
// This is a package-level variable that tests overwrite, which is safe only because the tests
// that do (withStoreSteps) never call t.Parallel and restore the original with t.Cleanup, and
// no test that runs in parallel reaches openStore. A test that replaced it while another
// ran in parallel would race; do not make one, and do not add t.Parallel to those.
var storeSteps = realStoreOps()

// checkMigrateDatabaseTLS applies the strict TLS rule to ANBUD_MIGRATE_DSN when it is
// set: the owner's password goes to a host that is not loopback only if the server's
// certificate is verified, exactly as ANBUD_DSN's does. The error names
// ANBUD_MIGRATE_DSN and the rule and never echoes the DSN.
func checkMigrateDatabaseTLS(cfg config.Sync) error {
	if !cfg.MigrateDatabaseURL.IsSet() {
		return nil
	}
	return db.CheckTLSFor(config.EnvMigrateDSN, cfg.MigrateDatabaseURL.Reveal())
}

// openStore returns the pool the engine writes through, with the migrations applied,
// or the exit code of the start that could not go on, having said why on stderr.
func openStore(ctx context.Context, cfg config.Sync, stderr io.Writer) (*pgxpool.Pool, int) {
	return openStoreWith(ctx, cfg, stderr, storeSteps)
}

// openStoreWith is openStore with its steps given. In order:
//
//  1. ANBUD_DSN is opened. This is the pool the engine writes through.
//  2. When ANBUD_MIGRATE_DSN is set it is opened too, and its role is checked
//     (db.CheckOwnerRole) before it does anything: a superuser, or a role that holds
//     CREATEROLE, CREATEDB or BYPASSRLS or a predefined role that reaches files or
//     programs, directly or through a role it belongs to, ends the start with exit 1 when
//     the host is not loopback and costs one warning line on stderr when it is. Its
//     ownership of the tables is not a reason: it owns them by design.
//  3. The migrations run through that pool, which is closed again before the engine
//     starts. When ANBUD_MIGRATE_DSN is unset the migrations run through ANBUD_DSN's
//     pool, as they always did, and that role, which is then the owner, is judged by
//     the stricter runtime check of the next step.
//  4. The role of the engine's pool is checked (db.CheckRuntimeRole), after the
//     migrations, because "owns a table" is only true once the tables exist. A role
//     that is too privileged ends the start with exit 1 when the host is not
//     loopback and costs one warning line on stderr when it is.
func openStoreWith(ctx context.Context, cfg config.Sync, stderr io.Writer, ops storeOps) (*pgxpool.Pool, int) {
	dsn := cfg.DatabaseURL.Reveal()
	pool, err := ops.open(ctx, dsn)
	if err != nil {
		fatalf(stderr, "could not open the database; check ANBUD_DSN")
		return nil, cancelledOr(ctx, exitFailed)
	}

	migrator := pool
	if cfg.MigrateDatabaseURL.IsSet() {
		migrateDSN := cfg.MigrateDatabaseURL.Reveal()
		if migrator, err = ops.open(ctx, migrateDSN); err != nil {
			pool.Close()
			fatalf(stderr, "could not open the database for the migrations; check "+config.EnvMigrateDSN)
			return nil, cancelledOr(ctx, exitFailed)
		}
		verdict, err := ops.checkOwner(ctx, migrator, migrateDSN)
		switch {
		case errors.Is(err, db.ErrOwnerTooPrivileged):
			migrator.Close()
			pool.Close()
			fatalf(stderr, db.ErrOwnerTooPrivileged.Error())
			return nil, exitUsage
		case err != nil:
			migrator.Close()
			pool.Close()
			fatalf(stderr, db.ErrOwnerCheckFailed.Error())
			return nil, cancelledOr(ctx, exitFailed)
		case verdict == db.RoleWarn:
			warnf(stderr, db.OwnerWarning)
		}
	}
	err = ops.migrate(ctx, migrator)
	if migrator != pool {
		migrator.Close()
	}
	if err != nil {
		pool.Close()
		fatalf(stderr, "could not apply the database migrations")
		return nil, cancelledOr(ctx, exitFailed)
	}

	verdict, err := ops.checkRole(ctx, pool, dsn)
	switch {
	case errors.Is(err, db.ErrRoleTooPrivileged):
		pool.Close()
		fatalf(stderr, db.ErrRoleTooPrivileged.Error())
		return nil, exitUsage
	case err != nil:
		pool.Close()
		fatalf(stderr, db.ErrRoleCheckFailed.Error())
		return nil, cancelledOr(ctx, exitFailed)
	case verdict == db.RoleWarn:
		warnf(stderr, db.RoleWarning)
	}
	return pool, exitDone
}

// warnf prints one warning line, in the form of fatalf's, for a start that goes on.
func warnf(w io.Writer, msg string) {
	_, _ = fmt.Fprintln(w, "doffin-sync: "+msg)
}
