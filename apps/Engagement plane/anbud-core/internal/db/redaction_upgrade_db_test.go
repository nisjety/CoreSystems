package db_test

import (
	"bytes"
	"slices"
	"testing"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// A database that holds notices already, migrated to 0005, takes migration 0006 as it is:
// every row passes the replaced checks, none is touched, the view shows them all, and the
// function erases one of them. Needs a throwaway server: run with make test-db.
func TestMigration0006UpgradesADatabaseThatAlreadyHoldsNotices(t *testing.T) {
	rs := dbtest.RoleDatabase(t) // the roles, and no migration yet
	admin := dbtest.Pool(t, rs.AdminDSN)
	migrator := dbtest.Pool(t, rs.MigratorDSN)
	ctx := roleCtx(t)

	apply := func(from, to int) {
		t.Helper()
		tx, err := migrator.Begin(ctx)
		if err != nil {
			t.Fatalf("begin: %s", dbtest.Describe(err))
		}
		defer func() { _ = tx.Rollback(ctx) }()
		if from == 1 {
			if _, err := tx.Exec(ctx, db.SchemaMigrationsDDL); err != nil {
				t.Fatalf("schema_migrations: %s", dbtest.Describe(err))
			}
		}
		for version := from; version <= to; version++ {
			name, body := db.EmbeddedMigration(t, version)
			if err := db.ApplyOne(ctx, tx, version, name, body); err != nil {
				t.Fatalf("migration %s: %s", name, dbtest.Describe(err))
			}
		}
		if err := tx.Commit(ctx); err != nil {
			t.Fatalf("commit migrations %d to %d: %s", from, to, dbtest.Describe(err))
		}
	}
	apply(1, 5)

	// What a database that ran the sync holds: a run, notices with and without contact
	// details, a search page and the attempts, written as the owner (the role model of the
	// application is not what is tested here).
	var run, held, plain int64
	if err := migrator.QueryRow(ctx, beginRunSQL).Scan(&run); err != nil {
		t.Fatalf("begin a run: %s", dbtest.Describe(err))
	}
	if err := migrator.QueryRow(ctx, insertNoticeSQL, "2026-100001", contactBody(""), run).Scan(&held); err != nil {
		t.Fatalf("store a notice: %s", dbtest.Describe(err))
	}
	if err := migrator.QueryRow(ctx, insertNoticeSQL, "2026-100002", []byte("<ContractNotice/>"), run).Scan(&plain); err != nil {
		t.Fatalf("store a second notice: %s", dbtest.Describe(err))
	}
	for _, id := range []struct {
		doffin string
		doc    int64
	}{{"2026-100001", held}, {"2026-100002", plain}} {
		if _, err := migrator.Exec(ctx, insertStoredAttemptSQL, run, id.doffin, id.doc); err != nil {
			t.Fatalf("store an attempt: %s", dbtest.Describe(err))
		}
	}
	if _, err := migrator.Exec(ctx, seedPageSQL, run, []byte(`{"hits":[]}`)); err != nil {
		t.Fatalf("store a search page: %s", dbtest.Describe(err))
	}
	// The columns 0006 adds do not exist yet: read the notice with the 0005 column list.
	var heldBody []byte
	var heldHash string
	if err := migrator.QueryRow(ctx, `SELECT body, body_sha256 FROM raw_documents WHERE id = $1`, held).Scan(&heldBody, &heldHash); err != nil {
		t.Fatalf("read the notice before the upgrade: %s", dbtest.Describe(err))
	}

	apply(6, 6)
	if err := db.ApplyMigrations(ctx, migrator); err != nil {
		t.Fatalf("ApplyMigrations after the upgrade: %s", dbtest.Describe(err))
	}

	// Nothing was touched. The three checks 0006 adds were added NOT VALID, so the upgrade did
	// not hash every body under an exclusive lock. Every old row already satisfied the
	// stronger checks of 0002, and validating them afterwards, which takes a lock that does
	// not block the sync, shows that none of them can fail.
	if got := scanInt(ctx, t, admin, `SELECT count(*) FROM raw_documents WHERE redacted_at IS NULL AND octet_length(body) > 0`); got != 2 {
		t.Errorf("%d notices are held after the upgrade, want 2", got)
	}
	notValid := scanStrings(ctx, t, admin, `SELECT conname::text FROM pg_constraint WHERE conrelid = 'raw_documents'::regclass AND contype = 'c' AND NOT convalidated ORDER BY conname`)
	if want := []string{"raw_documents_body_check", "raw_documents_check", "raw_documents_redacted_is_empty"}; !slices.Equal(notValid, want) {
		t.Errorf("the checks not validated after the upgrade are %v, want exactly %v", notValid, want)
	}
	for _, name := range notValid {
		if _, err := admin.Exec(ctx, `ALTER TABLE raw_documents VALIDATE CONSTRAINT `+name); err != nil {
			t.Errorf("a notice stored before the upgrade fails %s: %s", name, dbtest.Describe(err))
		}
	}
	if got := scanInt(ctx, t, admin, `SELECT count(*) FROM pg_constraint WHERE conrelid = 'raw_documents'::regclass AND contype = 'c' AND NOT convalidated`); got != 0 {
		t.Errorf("%d CHECK constraints of raw_documents are still not validated after VALIDATE CONSTRAINT", got)
	}
	if got := scanInt(ctx, t, admin, `SELECT count(*) FROM raw_documents_live`); got != 2 {
		t.Errorf("raw_documents_live shows %d rows after the upgrade, want both notices", got)
	}

	// The function works on a row that was stored before it existed.
	env := redactionEnvOn(t, rs, admin)
	var tomb int64
	if err := env.dba.QueryRow(ctx, redactSQL, held, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("redact a notice stored before the upgrade: %s", dbtest.Describe(err))
	}
	after := readDoc(ctx, t, admin, held)
	if len(after.Body) != 0 || after.RedactedAt == nil || after.BodySHA256 != heldHash {
		t.Error("the old notice is not redacted under its original hash")
	}
	if untouched := readDoc(ctx, t, admin, plain); len(untouched.Body) == 0 || untouched.RedactedAt != nil {
		t.Error("the other old notice was changed")
	}
	if got := scanInt(ctx, t, admin, `SELECT original_size FROM raw_redactions WHERE id = $1`, tomb); got != int64(len(heldBody)) || !bytes.Contains(heldBody, []byte(markerMail)) {
		t.Errorf("the tombstone records %d bytes for a body of %d", got, len(heldBody))
	}
}
