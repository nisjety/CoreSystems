// Package dbtest is the gate of this service's tests that need the REAL Data Plane
// schema: Postgres 16 with infra/postgres/init.sql and every migration applied by the
// real migrator, which is what MIGRATED_TEST_DATABASE_URL points at
// (tests/scripts/run-postgres-contract-tests.sh builds it and provides the variable).
//
// Only test files import this package.
//
// The pool is the SUPERUSER the services connect as, so row-level security is inert on
// it, exactly as in production. A test that proves tenant isolation must therefore go
// through the code under test (policy.Store, gates.Checker: both run inside the real
// orgscope helper, which drops to dataplane_app) or drop to the role itself in a
// transaction. On any other path RLS proves nothing.
package dbtest

import (
	"context"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Gate is the environment variable that carries the migrated database.
const Gate = "MIGRATED_TEST_DATABASE_URL"

// URL returns the connection URL of the migrated test database, after the checks that
// keep a test from ever touching a database that is not disposable, or ends the test.
//
// When the variable is absent the test SKIPS, and FAILS when REQUIRE_CONTRACT is set: a
// contract that silently skips proves nothing (the 2026-10-01 live proof found six defects
// that skipped suites had hidden). The skip reason names the variable, which is how
// tests/scripts/run-postgres-contract-tests.sh recognizes a database-gated test, and that
// runner (the postgres-contract-tests CI job) sets REQUIRE_CONTRACT=1 with a database, so in
// CI these tests cannot skip. CI alone is deliberately NOT a reason to fail: the plain
// go-build job runs `go test ./...` on this module with no database at all, by design.
//
// The URL is never printed: it carries a password.
func URL(t *testing.T) string {
	t.Helper()
	raw := strings.TrimSpace(os.Getenv(Gate))
	if raw == "" {
		if os.Getenv("REQUIRE_CONTRACT") != "" {
			t.Fatal(Gate + " is not set while REQUIRE_CONTRACT is: a database contract that cannot run must fail, never skip")
		}
		t.Skip(Gate + " is not set: point it at a disposable local Postgres 16 with the real migrations applied (tests/scripts/run-postgres-contract-tests.sh)")
	}
	parsed, err := url.Parse(raw)
	if err != nil || (parsed.Scheme != "postgres" && parsed.Scheme != "postgresql") {
		t.Fatal(Gate + " must be a postgres:// URL")
	}
	host := parsed.Hostname()
	local := host == "localhost" || host == "127.0.0.1" || host == "::1" ||
		(host == "" && strings.HasPrefix(parsed.Query().Get("host"), "/"))
	if !local {
		t.Fatalf("refusing a non-local test database host %q", host)
	}
	if name := strings.TrimPrefix(parsed.Path, "/"); !strings.HasSuffix(name, "_test") {
		t.Fatalf("refusing database %q: the name of a disposable test database must end in _test", name)
	}
	return raw
}

// Pool returns a pool on the migrated test database, or ends the test (see URL).
func Pool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	raw := URL(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, raw)
	if err != nil {
		t.Fatalf("connect to the migrated test database: %v", err)
	}
	t.Cleanup(pool.Close)
	if err := pool.Ping(ctx); err != nil {
		t.Fatalf("ping the migrated test database: %v", err)
	}
	return pool
}

// Exec runs a statement as the superuser and fails the test on error.
func Exec(t *testing.T, pool *pgxpool.Pool, sql string, args ...any) {
	t.Helper()
	if _, err := pool.Exec(context.Background(), sql, args...); err != nil {
		t.Fatalf("exec %q: %v", firstLine(sql), err)
	}
}

// Int runs a query that returns one integer.
func Int(t *testing.T, pool *pgxpool.Pool, sql string, args ...any) int64 {
	t.Helper()
	var n int64
	if err := pool.QueryRow(context.Background(), sql, args...).Scan(&n); err != nil {
		t.Fatalf("query %q: %v", firstLine(sql), err)
	}
	return n
}

func firstLine(sql string) string {
	sql = strings.TrimSpace(sql)
	if i := strings.IndexByte(sql, '\n'); i >= 0 {
		sql = sql[:i]
	}
	return sql
}
