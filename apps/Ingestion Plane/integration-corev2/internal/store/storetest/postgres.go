package storetest

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/integration-corev2/internal/db"
)

// PostgresURLEnv names the variable that points the database tests at a DISPOSABLE database
// (its name must contain "test" or "tmp"). A test that needs one skips without it, and FAILS
// when REQUIRE_CONTRACT or CI is set: a database test that quietly skips proves nothing.
const PostgresURLEnv = "INTEGRATION_TEST_DATABASE_URL"

// RequirePostgres returns the disposable database URL, or skips (fails under REQUIRE_CONTRACT
// or CI).
func RequirePostgres(t *testing.T) string {
	t.Helper()
	databaseURL := strings.TrimSpace(os.Getenv(PostgresURLEnv))
	if databaseURL != "" {
		return databaseURL
	}
	if os.Getenv("REQUIRE_CONTRACT") != "" || os.Getenv("CI") != "" {
		t.Fatalf("%s is not set but REQUIRE_CONTRACT or CI is: a database test that skips proves nothing", PostgresURLEnv)
	}
	t.Skipf("%s is not set", PostgresURLEnv)
	return ""
}

// IsolatedPostgres opens a pool whose search_path is a fresh schema of the disposable database,
// so a test owns its tables outright: it can apply migrations, drop and re-apply them, and run
// in parallel with another package's tests against the same database. The schema is dropped
// when the test ends. With migrate true every migration is applied first.
func IsolatedPostgres(t *testing.T, migrate bool) *pgxpool.Pool {
	t.Helper()
	databaseURL := RequirePostgres(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	admin, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		t.Fatalf("open disposable postgres: %v", err)
	}
	var databaseName string
	if err := admin.QueryRow(ctx, `SELECT current_database()`).Scan(&databaseName); err != nil {
		admin.Close()
		t.Fatalf("read database name: %v", err)
	}
	if normalized := strings.ToLower(databaseName); !strings.Contains(normalized, "test") && !strings.Contains(normalized, "tmp") {
		admin.Close()
		t.Fatalf("refusing to run against non-disposable database %q (its name must contain test or tmp)", databaseName)
	}

	schema := "zdr_t_" + strings.ReplaceAll(uuid.NewString(), "-", "")[:20]
	if _, err := admin.Exec(ctx, `CREATE SCHEMA `+schema); err != nil {
		admin.Close()
		t.Fatalf("create test schema: %v", err)
	}
	config, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		admin.Close()
		t.Fatalf("parse database url: %v", err)
	}
	config.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		admin.Close()
		t.Fatalf("open schema pool: %v", err)
	}
	t.Cleanup(func() {
		pool.Close()
		cleanup, cancelCleanup := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancelCleanup()
		_, _ = admin.Exec(cleanup, `DROP SCHEMA IF EXISTS `+schema+` CASCADE`)
		admin.Close()
	})
	if migrate {
		if err := db.ApplyMigrations(ctx, pool); err != nil {
			t.Fatalf("apply migrations: %v", err)
		}
	}
	return pool
}
