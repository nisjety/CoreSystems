package postgrescontract

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// ── gating ───────────────────────────────────────────────────────────────────

// requireDatabase returns the admin URL of a disposable Postgres, or ends the
// test. When REQUIRE_CONTRACT or CI is set an absent database is a FAILURE: a
// contract that silently skips is the failure mode that hid six defects in the
// 2026-10-01 live proof.
//
// The URL is never printed: it carries a password.
func requireDatabase(t *testing.T) *url.URL {
	t.Helper()
	raw := strings.TrimSpace(os.Getenv("TEST_DATABASE_URL"))
	if raw == "" {
		if os.Getenv("REQUIRE_CONTRACT") != "" || os.Getenv("CI") != "" {
			t.Fatal("TEST_DATABASE_URL is not set while REQUIRE_CONTRACT or CI is: a database contract that cannot run must fail, never skip")
		}
		t.Skip("TEST_DATABASE_URL is not set: point it at a disposable local Postgres 16 whose database name ends in _test (see doc.go)")
	}
	u, err := url.Parse(raw)
	if err != nil || (u.Scheme != "postgres" && u.Scheme != "postgresql") {
		t.Fatal("TEST_DATABASE_URL must be a postgres:// URL")
	}
	host := u.Hostname()
	local := host == "localhost" || host == "127.0.0.1" || host == "::1" ||
		(host == "" && strings.HasPrefix(u.Query().Get("host"), "/"))
	if !local {
		t.Fatalf("refusing non-local test database host %q", host)
	}
	if name := strings.TrimPrefix(u.Path, "/"); !strings.HasSuffix(name, "_test") {
		t.Fatalf("refusing database %q: the name of a disposable test database must end in _test", name)
	}
	return u
}

func withDatabase(admin *url.URL, name string) string {
	c := *admin
	c.Path = "/" + name
	c.RawPath = ""
	return c.String()
}

// ── scratch databases ────────────────────────────────────────────────────────

// scratchDB is one throwaway database on the disposable server. Each test that
// needs its own migration history gets one; the read-mostly tests share one.
type scratchDB struct {
	name string
	url  string
}

var (
	sharedMu    sync.Mutex
	sharedDB    *scratchDB
	sharedAdmin *url.URL
	sharedNames []string
)

func randomDatabaseName() string {
	b := make([]byte, 6)
	if _, err := rand.Read(b); err != nil {
		panic(err)
	}
	return "cr_t6_" + hex.EncodeToString(b) + "_test"
}

func dropDatabase(admin *url.URL, name string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	conn, err := pgx.Connect(ctx, admin.String())
	if err != nil {
		return err
	}
	defer conn.Close(ctx)
	_, err = conn.Exec(ctx, "DROP DATABASE IF EXISTS "+pgx.Identifier{name}.Sanitize()+" WITH (FORCE)")
	return err
}

// createScratch creates an empty database and applies the baseline. owned
// databases are dropped when the test ends; shared ones when the run ends.
func createScratch(t *testing.T, owned bool) *scratchDB {
	t.Helper()
	admin := requireDatabase(t)
	ctx, cancel := context.WithTimeout(t.Context(), 60*time.Second)
	defer cancel()

	conn, err := pgx.Connect(ctx, admin.String())
	if err != nil {
		t.Fatalf("connect to the test server: %v", err)
	}
	defer conn.Close(ctx)

	name := randomDatabaseName()
	if _, err := conn.Exec(ctx, "CREATE DATABASE "+pgx.Identifier{name}.Sanitize()); err != nil {
		t.Fatalf("create scratch database (the test role needs CREATEDB): %v", err)
	}
	if owned {
		t.Cleanup(func() {
			if err := dropDatabase(admin, name); err != nil {
				t.Logf("could not drop scratch database %s: %v", name, err)
			}
		})
	} else {
		sharedAdmin = admin
		sharedNames = append(sharedNames, name)
	}
	db := &scratchDB{name: name, url: withDatabase(admin, name)}
	applyBaseline(t, db)
	return db
}

// newScratchDatabase is a database only this test uses.
func newScratchDatabase(t *testing.T) *scratchDB {
	t.Helper()
	return createScratch(t, true)
}

// migratedDatabase is the one database, migrated through CR-T6, that the
// read-mostly tests share. Tests keep to their own org ids so they cannot
// disturb each other.
func migratedDatabase(t *testing.T) *scratchDB {
	t.Helper()
	sharedMu.Lock()
	defer sharedMu.Unlock()
	if sharedDB != nil {
		return sharedDB
	}
	db := createScratch(t, false)
	migrate(t, db, "up", "--to", cprVersion)
	sharedDB = db
	return db
}

// applyBaseline is the fresh-database path production uses: Postgres'
// docker-entrypoint-initdb runs infra/postgres/init.sql once, and the migrator
// then applies every migration on top of it.
func applyBaseline(t *testing.T, db *scratchDB) {
	t.Helper()
	initSQL, err := os.ReadFile(filepath.Join(dataPlaneRoot(t), "infra", "postgres", "init.sql"))
	if err != nil {
		t.Fatalf("read init.sql: %v", err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 60*time.Second)
	defer cancel()
	conn, err := pgx.Connect(ctx, db.url)
	if err != nil {
		t.Fatalf("connect to the scratch database: %v", err)
	}
	defer conn.Close(ctx)

	// The canonical context hash is over UTF-8 bytes (convert_to(..., 'UTF8')). On a
	// database in another encoding the hash tests would fail for a reason that has
	// nothing to do with the migration, so say so up front.
	var encoding string
	if err := conn.QueryRow(ctx, "SELECT pg_encoding_to_char(encoding) FROM pg_database WHERE datname = current_database()").Scan(&encoding); err != nil {
		t.Fatalf("read the scratch database encoding: %v", err)
	}
	if encoding != "UTF8" {
		t.Fatalf("the test server creates %s databases; the contract needs UTF8 (initdb --encoding=UTF8)", encoding)
	}

	// No arguments: pgx uses the simple protocol, so the whole file runs as one
	// multi-statement batch exactly as the migrator runs a migration.
	if _, err := conn.Exec(ctx, string(initSQL)); err != nil {
		t.Fatalf("apply init.sql: %v", err)
	}
}

func TestMain(m *testing.M) {
	code := m.Run()
	for _, name := range sharedNames {
		if err := dropDatabase(sharedAdmin, name); err != nil {
			fmt.Fprintf(os.Stderr, "could not drop shared scratch database %s: %v\n", name, err)
		}
	}
	if migratorDir != "" {
		_ = os.RemoveAll(migratorDir)
	}
	os.Exit(code)
}

// ── the real migrator ────────────────────────────────────────────────────────

var (
	migratorOnce sync.Once
	migratorPath string
	migratorDir  string
	migratorErr  error
)

// migratorBinary builds tools/migrator from source, once per run, with the
// same flags as its Dockerfile. The tests then drive that binary through its
// own command line, so a change to how it orders, wraps or records migrations
// is exercised, not assumed.
func migratorBinary(t *testing.T) string {
	t.Helper()
	migratorOnce.Do(func() {
		dir, err := os.MkdirTemp("", "dataplane-migrator-")
		if err != nil {
			migratorErr = err
			return
		}
		migratorDir = dir
		out := filepath.Join(dir, "migrator")
		if runtime.GOOS == "windows" {
			out += ".exe"
		}
		build := exec.Command("go", "build", "-trimpath", "-o", out, ".")
		build.Dir = filepath.Join(dataPlaneRoot(t), "tools", "migrator")
		build.Env = append(os.Environ(), "CGO_ENABLED=0")
		if output, err := build.CombinedOutput(); err != nil {
			migratorErr = fmt.Errorf("build tools/migrator: %v\n%s", err, output)
			return
		}
		migratorPath = out
	})
	if migratorErr != nil {
		t.Fatal(migratorErr)
	}
	return migratorPath
}

// migrate runs the real migrator against the scratch database.
func migrate(t *testing.T, db *scratchDB, args ...string) string {
	t.Helper()
	full := append(append([]string{}, args...), "--migrations-dir", migrationsDir(t))
	cmd := exec.CommandContext(t.Context(), migratorBinary(t), full...)
	cmd.Env = append(os.Environ(), "DATABASE_URL="+db.url)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("migrator %s failed: %v\n%s", strings.Join(args, " "), err, out)
	}
	return string(out)
}

// ── connections and queries ──────────────────────────────────────────────────

// openPool connects as the migration owner, a superuser, exactly as every Data
// Plane service does. Simple protocol: the schema changes under this pool
// (migrations, rollbacks), which would invalidate prepared-statement caches.
func openPool(t *testing.T, db *scratchDB) *pgxpool.Pool {
	t.Helper()
	cfg, err := pgxpool.ParseConfig(db.url)
	if err != nil {
		t.Fatalf("parse scratch database URL: %v", err)
	}
	cfg.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
	cfg.MaxConns = 4
	pool, err := pgxpool.NewWithConfig(t.Context(), cfg)
	if err != nil {
		t.Fatalf("open pool: %v", err)
	}
	t.Cleanup(pool.Close)
	return pool
}

func execSQL(t *testing.T, pool *pgxpool.Pool, sql string) {
	t.Helper()
	if _, err := pool.Exec(t.Context(), sql); err != nil {
		t.Fatalf("%v\nSQL: %s", err, sql)
	}
}

func queryInt(t *testing.T, pool *pgxpool.Pool, sql string) int64 {
	t.Helper()
	var n int64
	if err := pool.QueryRow(t.Context(), sql).Scan(&n); err != nil {
		t.Fatalf("%v\nSQL: %s", err, sql)
	}
	return n
}

// queryString returns the first column of the first row as text; NULL is
// "<NULL>" so it can never be mistaken for an empty string.
func queryString(t *testing.T, pool *pgxpool.Pool, sql string) string {
	t.Helper()
	var s *string
	if err := pool.QueryRow(t.Context(), sql).Scan(&s); err != nil {
		t.Fatalf("%v\nSQL: %s", err, sql)
	}
	if s == nil {
		return "<NULL>"
	}
	return *s
}

// queryLines returns the first column of every row as text.
func queryLines(t *testing.T, pool *pgxpool.Pool, sql string) []string {
	t.Helper()
	rows, err := pool.Query(t.Context(), sql)
	if err != nil {
		t.Fatalf("%v\nSQL: %s", err, sql)
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var s *string
		if err := rows.Scan(&s); err != nil {
			t.Fatalf("scan: %v\nSQL: %s", err, sql)
		}
		if s == nil {
			out = append(out, "<NULL>")
		} else {
			out = append(out, *s)
		}
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("%v\nSQL: %s", err, sql)
	}
	return out
}

// expectPgError asserts err is a Postgres error with the given SQLSTATE and,
// when constraint is non-empty, the given constraint name. A nil err fails: it
// means the database accepted a row it must reject.
func expectPgError(t *testing.T, err error, code, constraint string) {
	t.Helper()
	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) {
		t.Fatalf("want Postgres error %s %s, got: %v", code, constraint, err)
	}
	if pgErr.Code != code || (constraint != "" && pgErr.ConstraintName != constraint) {
		t.Fatalf("want %s %s, got %s %q: %s", code, constraint, pgErr.Code, pgErr.ConstraintName, pgErr.Message)
	}
}

// ── SQL literals and fixtures ────────────────────────────────────────────────

func q(s string) string   { return "'" + strings.ReplaceAll(s, "'", "''") + "'" }
func sp(s string) *string { return &s }

func qn(s *string) string {
	if s == nil {
		return "NULL"
	}
	return q(*s)
}

func sha256hex(s string) string {
	sum := sha256.Sum256([]byte(s))
	return hex.EncodeToString(sum[:])
}

func insertDocument(t *testing.T, pool *pgxpool.Pool, id, org, classification string, deleted bool) {
	t.Helper()
	deletedAt := "NULL"
	if deleted {
		deletedAt = "now()"
	}
	execSQL(t, pool, fmt.Sprintf(
		`INSERT INTO documents (document_id, org_id, source, type, title, content, zdr_classification, deleted_at)
		 VALUES (%s, %s, 'contract-test', 'note', %s, 'content', %s, %s)`,
		q(id), q(org), q("title "+id), q(classification), deletedAt))
}

// unitSpec is one knowledge unit. Zero values are the common case: an indexed
// unit with no context and no status.
type unitSpec struct {
	id, doc, org string
	embedding    string  // embedding_status; default "done"
	context      *string // chunk_context
	status       *string // context_status (needs the CR-T6 migration)
	class        *string // context_error_class
	hash         *string // embedded_context_hash
	inputHash    *string // context_input_hash
	model        *string // embedding_model
	attempts     int     // context_attempts
}

var chunkSeq atomic.Int64

// unitInsertSQL builds the INSERT. updated_at is set explicitly, to a value in
// the past, so a test can prove a later statement did not move it (the
// BEFORE UPDATE trigger would). Units that carry a context are stamped as the
// live corpus is: updated_at after embedded_at.
func unitInsertSQL(u unitSpec, provenance bool) string {
	embedding := u.embedding
	if embedding == "" {
		embedding = "done"
	}
	hasContext := u.context != nil && strings.TrimSpace(*u.context) != ""
	promptVersion := "NULL"
	if hasContext {
		promptVersion = q("ctx-v1")
	}
	embeddedAt := "NULL"
	if embedding == "done" {
		embeddedAt = "'2026-09-01T10:00:00Z'"
	}
	updatedAt := "'2026-09-01T10:00:00Z'"
	if hasContext {
		updatedAt = "'2026-09-01T10:05:00Z'"
	}
	cols := []string{
		"knowledge_id", "document_id", "org_id", "chunk_index", "text", "embedding_status",
		"chunk_context", "context_prompt_version", "embedding_model", "embedded_at", "created_at", "updated_at",
	}
	vals := []string{
		q(u.id), q(u.doc), q(u.org), fmt.Sprint(chunkSeq.Add(1)), q("chunk text " + u.id), q(embedding),
		qn(u.context), promptVersion, qn(u.model), embeddedAt, "'2026-09-01T09:00:00Z'", updatedAt,
	}
	if provenance {
		cols = append(cols, "context_status", "context_error_class", "embedded_context_hash", "context_input_hash", "context_attempts")
		vals = append(vals, qn(u.status), qn(u.class), qn(u.hash), qn(u.inputHash), fmt.Sprint(u.attempts))
	}
	return "INSERT INTO knowledge_units (" + strings.Join(cols, ", ") + ") VALUES (" + strings.Join(vals, ", ") + ")"
}

func insertUnit(t *testing.T, pool *pgxpool.Pool, u unitSpec, provenance bool) {
	t.Helper()
	execSQL(t, pool, unitInsertSQL(u, provenance))
}
