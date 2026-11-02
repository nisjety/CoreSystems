package db_test

import (
	"context"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Shared by the redaction tests (redaction_*_db_test.go). They need a throwaway server:
// run them with make test-db. Each test gets a database of its own, prepared the way a
// DBA prepares one (scripts/roles.sql, then the migrations as anbud_migrator), with a
// login role in anbud_dba to redact as.

const (
	dbaLogin    = "anbud_test_redactor"
	nobodyLogin = "anbud_test_nobody"

	redactSQL = `SELECT anbud_redact_raw_document($1, $2, $3)`

	// SQLSTATEs of the redaction function and its guards, beyond the ones of
	// roles_boundary_db_test.go (stateDenied 42501, stateRestrict 23001, stateNotReady 55000).
	stateNullArg         = "22004" // null_value_not_allowed
	stateBadArg          = "22023" // invalid_parameter_value
	stateNoSuchDocument  = "P0002" // no_data_found
	stateAlreadyRedacted = stateNotReady
	stateCheck           = "23514" // check_violation
	stateUnique          = "23505" // unique_violation
	stateNoReference     = "23503" // foreign_key_violation
	stateNotSupported    = "0A000" // TRUNCATE of a table another table references
	stateRaised          = "P0001" // raise_exception, the code of an ordinary RAISE EXCEPTION
	stateInternal        = "XX000" // internal_error: the function's own check that its UPDATE changed exactly one row

	// The door: a custom setting the redaction function sets, in its own transaction, for
	// the one row it is about to erase. The guard on raw_documents reads it.
	doorSetting = "anbud.redacting_raw_document"

	// Personal data the seeded notices hold. Nothing here is a real person: the domain is
	// reserved for examples and the number is not assigned.
	markerName  = "Kari Nordmann"
	markerMail  = "kari.nordmann@example.no"
	markerPhone = "+47 22 33 44 55"

	insertNoticeSQL        = `INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id) VALUES ('doffin', $1, 'application/xml', $2::bytea, encode(sha256($2::bytea), 'hex'), now(), $3) RETURNING id`
	insertStoredAttemptSQL = `INSERT INTO download_attempts (run_id, source, doffin_id, attempted_at, outcome, raw_document_id) VALUES ($1, 'doffin', $2, now(), 'stored', $3)`
)

// contactBody is a notice body that names a contact person, as eForms notices do.
func contactBody(extra string) []byte {
	return []byte(`<ContractNotice><ContactPoint><Name>` + markerName + `</Name><Telephone>` + markerPhone +
		`</Telephone><Email>` + markerMail + `</Email></ContactPoint>` + extra + `</ContractNotice>`)
}

// redactionEnv is a migrated role-model database with one caller of each kind.
type redactionEnv struct {
	rs     dbtest.RoleSet
	admin  *pgxpool.Pool // the server's own role, a superuser: it looks and arranges, never redacts
	owner  *pgxpool.Pool // anbud_migrator, which owns every object
	app    *pgxpool.Pool // anbud_app
	dba    *pgxpool.Pool // a login role that is a member of anbud_dba
	dbaDSN string
	run    int64 // a run that stays running, for seeding
}

func newRedactionEnv(t *testing.T) redactionEnv {
	t.Helper()
	rs, admin := migratedRoles(t)
	return redactionEnvOn(t, rs, admin)
}

// redactionEnvOn adds the callers to a database that is migrated and has the roles, however
// it got there.
func redactionEnvOn(t *testing.T, rs dbtest.RoleSet, admin *pgxpool.Pool) redactionEnv {
	t.Helper()
	ctx := roleCtx(t)
	dbaDSN := dbtest.CreateLoginRole(t, rs, dbaLogin, "", dbtest.RoleDBA)
	env := redactionEnv{
		rs:     rs,
		admin:  admin,
		owner:  dbtest.Pool(t, rs.MigratorDSN),
		app:    dbtest.Pool(t, rs.AppDSN),
		dba:    dbtest.Pool(t, dbaDSN),
		dbaDSN: dbaDSN,
	}
	if err := env.app.QueryRow(ctx, beginRunSQL).Scan(&env.run); err != nil {
		t.Fatalf("begin a run as the app role: %s", dbtest.Describe(err))
	}
	return env
}

// seedNotice stores a notice and its download attempt as the application does.
func (e redactionEnv) seedNotice(ctx context.Context, t *testing.T, doffinID string, body []byte) int64 {
	t.Helper()
	var id int64
	if err := e.app.QueryRow(ctx, insertNoticeSQL, doffinID, body, e.run).Scan(&id); err != nil {
		t.Fatalf("seed a notice as the app role: %s", dbtest.Describe(err))
	}
	if _, err := e.app.Exec(ctx, insertStoredAttemptSQL, e.run, doffinID, id); err != nil {
		t.Fatalf("seed its attempt as the app role: %s", dbtest.Describe(err))
	}
	return id
}

// docRow is a raw_documents row, every column of it.
type docRow struct {
	ID          int64
	Source      string
	DoffinID    string
	DataClass   string
	ContentType string
	Body        []byte
	BodySHA256  string
	FetchedAt   time.Time
	StoredAt    time.Time
	RunID       int64
	RedactedAt  *time.Time
}

func readDoc(ctx context.Context, t *testing.T, q queryRower, id int64) docRow {
	t.Helper()
	var r docRow
	err := q.QueryRow(ctx, `SELECT id, source, doffin_id, data_class, content_type, body, body_sha256, fetched_at, stored_at, run_id, redacted_at FROM raw_documents WHERE id = $1`, id).
		Scan(&r.ID, &r.Source, &r.DoffinID, &r.DataClass, &r.ContentType, &r.Body, &r.BodySHA256, &r.FetchedAt, &r.StoredAt, &r.RunID, &r.RedactedAt)
	if err != nil {
		t.Fatalf("read raw document %d: %s", id, dbtest.Describe(err))
	}
	return r
}

// queryRower is what a pool, a pooled connection and a transaction share.
type queryRower interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

func scanInt(ctx context.Context, t *testing.T, q queryRower, sql string, args ...any) int64 {
	t.Helper()
	var n int64
	if err := q.QueryRow(ctx, sql, args...).Scan(&n); err != nil {
		t.Fatalf("query %q: %s", sql, dbtest.Describe(err))
	}
	return n
}

// step is one statement of an attempt.
type step struct {
	sql  string
	args []any
}

func do(sql string, args ...any) step { return step{sql: sql, args: args} }

// try runs the steps inside a savepoint of tx and undoes them all, and says how the last
// one ended: "ok", or the SQLSTATE of the server's error. The earlier steps arrange the
// attempt and must succeed. Undoing them also undoes a transaction-local setting, so a
// door a step opens never outlives its attempt.
func try(ctx context.Context, t *testing.T, tx pgx.Tx, steps ...step) string {
	t.Helper()
	if _, err := tx.Exec(ctx, "SAVEPOINT attempt"); err != nil {
		t.Fatalf("savepoint: %s", dbtest.Describe(err))
	}
	result := stateOK
	for i, s := range steps {
		_, err := tx.Exec(ctx, s.sql, s.args...)
		if i < len(steps)-1 {
			if err != nil {
				t.Fatalf("arranging step %d (%q) failed: %s", i+1, s.sql, dbtest.Describe(err))
			}
			continue
		}
		result = dbtest.Outcome(err)
	}
	for _, stmt := range []string{"ROLLBACK TO SAVEPOINT attempt", "RELEASE SAVEPOINT attempt"} {
		if _, err := tx.Exec(ctx, stmt); err != nil {
			t.Fatalf("%s: %s", stmt, dbtest.Describe(err))
		}
	}
	return result
}

// connectLogging opens a session that asks the server to send its LOG messages, and
// returns a function that reads what arrived. RAISE LOG goes to the server log and, for a
// session that asks, to the client as well, which is how a test reads it.
func connectLogging(ctx context.Context, t *testing.T, dsn string) (*pgx.Conn, func() []string) {
	t.Helper()
	dbtest.MustBeThrowaway(t, dsn)
	cfg, err := pgx.ParseConfig(dsn)
	if err != nil {
		t.Fatal("dbtest: refusing an unparsable DSN")
	}
	var (
		mu   sync.Mutex
		logs []string
	)
	cfg.RuntimeParams["client_min_messages"] = "log"
	cfg.OnNotice = func(_ *pgconn.PgConn, n *pgconn.Notice) {
		mu.Lock()
		defer mu.Unlock()
		logs = append(logs, n.Severity+": "+n.Message)
	}
	conn, err := pgx.ConnectConfig(ctx, cfg)
	if err != nil {
		t.Fatalf("connect: %s", dbtest.Describe(err))
	}
	t.Cleanup(func() { _ = conn.Close(context.Background()) })
	return conn, func() []string {
		mu.Lock()
		defer mu.Unlock()
		return append([]string(nil), logs...)
	}
}
