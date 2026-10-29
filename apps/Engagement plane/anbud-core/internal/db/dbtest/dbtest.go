// Package dbtest gives database tests a throwaway database on the server named by
// ANBUD_TEST_DSN. The guard refuses any DSN whose database is not postgres and
// does not start with anbud_test, so a test run cannot reach anbud_core. Each
// test gets a fresh migrated database, dropped when the test ends. Tests never
// delete rows: the evidence tables refuse it (migration 0002). No message of this
// package carries a DSN or a part of one: a connection error quotes the host, the
// role and the database, so errors are reported through Describe.
package dbtest

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
)

// EnvVar names the variable that holds the throwaway server's DSN.
const EnvVar = "ANBUD_TEST_DSN"

// RequireVar names the variable that turns an empty EnvVar into a failure. The
// repository script scripts/test-db.sh sets it to 1, so a database run cannot
// pass by skipping its database tests.
const RequireVar = "ANBUD_REQUIRE_DB_TESTS"

const (
	allowedDatabase = "postgres"
	allowedPrefix   = "anbud_test"
	databasePrefix  = "anbud_test_"
	setupTimeout    = 60 * time.Second
	cleanupTimeout  = 30 * time.Second
	// maxConns matches db.Open, so ApplyMigrations sees the production pool size.
	maxConns = 10
)

var errRefused = errors.New("dbtest: refusing this DSN: its database must be postgres or start with anbud_test")

// CheckDSN reports whether dsn may host throwaway databases. The database name
// is the one pgx would connect to, so defaults from the environment count. The
// error never includes the DSN, because pgx errors can quote it.
func CheckDSN(dsn string) error {
	cfg, err := pgx.ParseConfig(dsn)
	if err != nil {
		return errors.New("dbtest: refusing an unparsable DSN")
	}
	if cfg.Database == allowedDatabase || strings.HasPrefix(cfg.Database, allowedPrefix) {
		return nil
	}
	return errRefused
}

// unsetDSNFails decides what an empty EnvVar means. Only the value 1 makes it a
// failure. Otherwise the test skips, as it always has, so a machine with no
// database still runs the pure tests.
func unsetDSNFails(require string) bool { return require == "1" }

// ServerDSN returns ANBUD_TEST_DSN after the guard. When the variable is unset it
// fails t if RequireVar is 1, and skips t otherwise. It fails t when the guard
// refuses the DSN. Messages name variables and never values.
func ServerDSN(t testing.TB) string {
	t.Helper()
	dsn := os.Getenv(EnvVar)
	if dsn == "" {
		if unsetDSNFails(os.Getenv(RequireVar)) {
			t.Fatalf("dbtest: %s is empty and %s=1, so this database test cannot run: set %s to a throwaway server's DSN, or unset %s to skip", EnvVar, RequireVar, EnvVar, RequireVar)
		}
		t.Skip(EnvVar + " not set")
	}
	if err := CheckDSN(dsn); err != nil {
		t.Fatal(err)
	}
	return dsn
}

// Open creates a fresh database on the test server, applies every migration to
// it, and returns a pool on it. Cleanup closes the pool and drops the database.
func Open(t testing.TB) *pgxpool.Pool {
	t.Helper()
	dsn := Database(t)
	// Cleanup runs last in, first out: Database registered the drop first, so the
	// pool registered here closes before its database is dropped.
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		t.Fatal("dbtest: refusing an unparsable DSN")
	}
	cfg.MaxConns = maxConns
	pool, err := pgxpool.NewWithConfig(context.Background(), cfg)
	if err != nil {
		t.Fatalf("dbtest: open pool: %s", Describe(err))
	}
	t.Cleanup(pool.Close)
	return pool
}

// Database creates a fresh database on the test server, applies every migration to
// it, and returns a DSN for it. Cleanup drops the database. Use it when a test needs
// a DSN, such as an end-to-end run of a command, rather than a pool. The server DSN
// must be a postgres:// URL, because the database name is set in its path.
func Database(t testing.TB) string {
	t.Helper()
	server := ServerDSN(t)
	name := newDatabaseName(t)

	ctx, cancel := context.WithTimeout(context.Background(), setupTimeout)
	defer cancel()
	createDatabase(ctx, t, server, name)
	t.Cleanup(func() { dropDatabase(t, server, name) })

	dsn := dsnForDatabase(t, server, name)
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		t.Fatal("dbtest: refusing an unparsable DSN")
	}
	cfg.MaxConns = maxConns
	pool, err := pgxpool.NewWithConfig(context.Background(), cfg)
	if err != nil {
		t.Fatalf("dbtest: open pool: %s", Describe(err))
	}
	defer pool.Close()

	if err := db.ApplyMigrations(ctx, pool); err != nil {
		t.Fatalf("dbtest: apply migrations: %s", Describe(err))
	}
	return dsn
}

// dsnForDatabase returns the server DSN with its database path replaced by name.
// It refuses any server DSN that is not a postgres:// URL. Its message never
// includes the DSN.
func dsnForDatabase(t testing.TB, server, name string) string {
	t.Helper()
	u, err := url.Parse(server)
	if err != nil || (u.Scheme != "postgres" && u.Scheme != "postgresql") {
		t.Fatalf("dbtest: %s must be a postgres:// URL for this helper", EnvVar)
	}
	u.Path = "/" + name
	return u.String()
}

// newDatabaseName returns a random name with the anbud_test_ prefix. It is hex
// only, so it needs no quoting beyond the identifier form.
func newDatabaseName(t testing.TB) string {
	t.Helper()
	var b [8]byte
	if _, err := rand.Read(b[:]); err != nil {
		t.Fatalf("dbtest: random database name: %v", err)
	}
	return databasePrefix + hex.EncodeToString(b[:])
}

func createDatabase(ctx context.Context, t testing.TB, server, name string) {
	t.Helper()
	admin, err := pgx.Connect(ctx, server)
	if err != nil {
		t.Fatalf("dbtest: connect to the test server: %s", Describe(err))
	}
	defer func() { _ = admin.Close(context.Background()) }()

	stmt := "CREATE DATABASE " + pgx.Identifier{name}.Sanitize() + " TEMPLATE template0"
	if _, err := admin.Exec(ctx, stmt); err != nil {
		t.Fatalf("dbtest: create a database: %s", Describe(err))
	}
}

// dropDatabase runs at cleanup. FORCE ends any session still attached.
func dropDatabase(t testing.TB, server, name string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), cleanupTimeout)
	defer cancel()
	admin, err := pgx.Connect(ctx, server)
	if err != nil {
		t.Errorf("dbtest: connect to drop a database: %s", Describe(err))
		return
	}
	defer func() { _ = admin.Close(context.Background()) }()

	stmt := "DROP DATABASE IF EXISTS " + pgx.Identifier{name}.Sanitize() + " WITH (FORCE)"
	if _, err := admin.Exec(ctx, stmt); err != nil {
		t.Errorf("dbtest: drop a database: %s", Describe(err))
	}
}
