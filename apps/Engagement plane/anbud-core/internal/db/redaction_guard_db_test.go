package db_test

import (
	"context"
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The guards of migration 0006 against a real server: what the trigger on raw_documents
// admits (the redaction and nothing else), the tombstone table, and the INSERT guard.
// Needs a throwaway server: run with make test-db.

const (
	// tombstoneByHandSQL is a tombstone written directly, as only the owner could. The
	// parameters are the document, a hash to use instead of its own (or NULL), a number to
	// add to its size, and seconds to add to now().
	tombstoneByHandSQL = `INSERT INTO raw_redactions (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by, redacted_at)
		SELECT d.id, d.doffin_id, COALESCE($2, d.body_sha256), octet_length(d.body) + $3::int, 'controller_decision', 'HAND-1', 'owner', now() + $4::int * interval '1 second'
		  FROM raw_documents d WHERE d.id = $1`
	openDoorSQL     = `SELECT set_config('` + doorSetting + `', $1, true)`
	narrowUpdateSQL = `UPDATE raw_documents SET body = ''::bytea, redacted_at = now() WHERE id = $1`
)

func narrow(id int64) step { return do(narrowUpdateSQL, id) }

func door(id, xid int64) step { return do(openDoorSQL, fmt.Sprintf("%d:%d", id, xid)) }

// The trigger on raw_documents refuses every UPDATE and every DELETE except one
// transformation: the row's body becomes empty and redacted_at is set, with every other
// column as it was, while a door that names this row and this transaction is open and a
// tombstone for the row, with its hash, its size and the same time, is already written.
// The owner is bound by it too. Each attempt below is one condition short of that, or
// changes one thing more; the control at the end meets all of them.
func TestRawDocumentsGuardAdmitsOnlyTheRedactionAndNothingWider(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)

	a := env.seedNotice(ctx, t, "2026-100001", contactBody("<A/>")) // gets the right tombstone
	b := env.seedNotice(ctx, t, "2026-100002", contactBody("<B/>")) // another row
	c := env.seedNotice(ctx, t, "2026-100003", contactBody("<C/>")) // a tombstone with another hash
	e := env.seedNotice(ctx, t, "2026-100004", contactBody("<E/>")) // a tombstone with another size
	d := env.seedNotice(ctx, t, "2026-100005", contactBody("<D/>")) // no tombstone
	f := env.seedNotice(ctx, t, "2026-100006", contactBody("<F/>")) // a tombstone made at another time

	tx, err := env.owner.Begin(ctx)
	if err != nil {
		t.Fatalf("begin as the owner: %s", dbtest.Describe(err))
	}
	defer func() { _ = tx.Rollback(ctx) }()
	xid := scanInt(ctx, t, tx, `SELECT txid_current()`)
	for _, s := range []step{
		do(tombstoneByHandSQL, a, nil, 0, 0),
		do(tombstoneByHandSQL, c, strings.Repeat("b", 64), 0, 0),
		do(tombstoneByHandSQL, e, nil, 1, 0),
		do(tombstoneByHandSQL, f, nil, 0, 1),
	} {
		if _, err := tx.Exec(ctx, s.sql, s.args...); err != nil {
			t.Fatalf("arrange a tombstone by hand as the owner: %s", dbtest.Describe(err))
		}
	}

	type attempt struct {
		name  string
		steps []step
	}
	cases := []attempt{
		{"an UPDATE with no door", []step{do(`UPDATE raw_documents SET content_type = 'changed' WHERE id = $1`, a)}},
		{"the redaction's own UPDATE with no door, by the owner", []step{narrow(a)}},
		{"a DELETE with no door", []step{do(`DELETE FROM raw_documents WHERE id = $1`, a)}},
		{"a DELETE with the door open for that row", []step{door(a, xid), do(`DELETE FROM raw_documents WHERE id = $1`, a)}},
		{"a door for another row", []step{door(b, xid), narrow(a)}},
		{"a door that names the row and not the transaction", []step{do(openDoorSQL, fmt.Sprint(a)), narrow(a)}},
		{"a door of another transaction", []step{door(a, xid-1), narrow(a)}},
		{"a row with no tombstone", []step{door(d, xid), narrow(d)}},
		{"a tombstone with another hash", []step{door(c, xid), narrow(c)}},
		{"a tombstone with another size", []step{door(e, xid), narrow(e)}},
		{"a tombstone made at another time", []step{door(f, xid), narrow(f)}},
		{"a body that is not empty", []step{door(a, xid), do(`UPDATE raw_documents SET body = 'x'::bytea, redacted_at = now() WHERE id = $1`, a)}},
		{"a body emptied and not marked", []step{door(a, xid), do(`UPDATE raw_documents SET body = ''::bytea WHERE id = $1`, a)}},
		{"a mark on a body that is kept", []step{door(a, xid), do(`UPDATE raw_documents SET redacted_at = now() WHERE id = $1`, a)}},
		{"two rows in one statement, one of them without a door", []step{door(a, xid), do(`UPDATE raw_documents SET body = ''::bytea, redacted_at = now() WHERE id IN ($1, $2)`, a, b)}},
	}

	// Every column but the two the redaction changes: the redaction with that column
	// changed as well is refused. A column a later migration adds fails the test until it
	// is listed here and compared by the guard.
	changed := map[string]string{
		"id":           `id = DEFAULT`,
		"source":       `source = 'other'`,
		"doffin_id":    `doffin_id = '2099-000001'`,
		"data_class":   `data_class = 'other'`,
		"content_type": `content_type = 'changed'`,
		"body_sha256":  `body_sha256 = repeat('a', 64)`,
		"fetched_at":   `fetched_at = fetched_at + interval '1 day'`,
		"stored_at":    `stored_at = stored_at + interval '1 day'`,
		"run_id":       `run_id = run_id + 1`,
	}
	columns := scanStrings(ctx, t, env.admin, `SELECT column_name::text FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'raw_documents' ORDER BY ordinal_position`)
	for _, col := range columns {
		if col == "body" || col == "redacted_at" {
			continue
		}
		set, ok := changed[col]
		if !ok {
			t.Errorf("raw_documents has a column %s that this test does not change together with the redaction: list it here, and make the guard compare it", col)
			continue
		}
		cases = append(cases, attempt{"the redaction with " + col + " changed as well", []step{door(a, xid), do(`UPDATE raw_documents SET body = ''::bytea, redacted_at = now(), `+set+` WHERE id = $1`, a)}})
	}
	for col := range changed {
		if !slices.Contains(columns, col) {
			t.Errorf("this test changes %s, which raw_documents no longer has", col)
		}
	}
	for _, c := range cases {
		if got := try(ctx, t, tx, c.steps...); got != stateRestrict {
			t.Errorf("%s = %s, want %s from the guard", c.name, got, stateRestrict)
		}
	}
	// TRUNCATE is refused too, the door open or not: by the server first, as a table
	// another table references cannot be truncated, and by the trigger otherwise.
	for _, open := range []bool{false, true} {
		steps := []step{do(`TRUNCATE raw_documents`)}
		if open {
			steps = []step{door(a, xid), do(`TRUNCATE raw_documents`)}
		}
		if got := try(ctx, t, tx, steps...); got != stateNotSupported && got != stateRestrict {
			t.Errorf("TRUNCATE (door open: %v) = %s, want %s or %s", open, got, stateNotSupported, stateRestrict)
		}
	}
	if got := scanInt(ctx, t, tx, `SELECT count(*) FROM raw_documents WHERE redacted_at IS NOT NULL OR octet_length(body) = 0`); got != 0 {
		t.Fatalf("%d rows were redacted or emptied by the refused attempts", got)
	}

	// Control: the transformation itself, with every condition met, goes through. Without
	// it the refusals above could all be a setup that never worked.
	for _, s := range []step{door(a, xid), narrow(a)} {
		if _, err := tx.Exec(ctx, s.sql, s.args...); err != nil {
			t.Fatalf("control: the redaction update with the door open and a tombstone was refused: %s", dbtest.Describe(err))
		}
	}
	if got := scanInt(ctx, t, tx, `SELECT count(*) FROM raw_documents WHERE id = $1 AND redacted_at IS NOT NULL AND octet_length(body) = 0`, a); got != 1 {
		t.Fatalf("control: the row is not redacted after the redaction update (%d)", got)
	}
	if _, err := tx.Exec(ctx, openDoorSQL, ""); err != nil {
		t.Fatalf("close the door: %s", dbtest.Describe(err))
	}

	// A redacted row is final: it cannot be marked again, put back, or deleted, door or no door.
	for name, steps := range map[string][]step{
		"clearing the mark":       {door(a, xid), do(`UPDATE raw_documents SET redacted_at = NULL WHERE id = $1`, a)},
		"putting a body back":     {door(a, xid), do(`UPDATE raw_documents SET body = 'x'::bytea WHERE id = $1`, a)},
		"marking it again":        {door(a, xid), do(`UPDATE raw_documents SET redacted_at = now() + interval '1 second' WHERE id = $1`, a)},
		"the redaction once more": {door(a, xid), narrow(a)},
		"a DELETE":                {door(a, xid), do(`DELETE FROM raw_documents WHERE id = $1`, a)},
	} {
		if got := try(ctx, t, tx, steps...); got != stateRestrict {
			t.Errorf("%s on a redacted row = %s, want %s from the guard", name, got, stateRestrict)
		}
	}

	// The view is no way round the guard: an UPDATE or DELETE through it is the same
	// statement on the table.
	for name, stmt := range map[string]string{
		"UPDATE through raw_documents_live": `UPDATE raw_documents_live SET content_type = 'changed' WHERE id = $1`,
		"DELETE through raw_documents_live": `DELETE FROM raw_documents_live WHERE id = $1`,
	} {
		if got := try(ctx, t, tx, do(stmt, b)); got != stateRestrict {
			t.Errorf("%s = %s, want %s from the guard", name, got, stateRestrict)
		}
	}
}

// A DBA who tries an UPDATE by hand is told where the one change is made, and neither
// refusal repeats anything of the row, which can hold personal data.
func TestARefusedUpdateNamesTheFunctionAndRepeatsNothingOfTheRow(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	env.seedNotice(ctx, t, "2026-100001", contactBody(""))

	_, update := env.owner.Exec(ctx, `UPDATE raw_documents SET content_type = 'changed'`)
	_, remove := env.owner.Exec(ctx, `DELETE FROM raw_documents`)
	for name, err := range map[string]error{"UPDATE": update, "DELETE": remove} {
		if dbtest.Outcome(err) != stateRestrict {
			t.Fatalf("%s by the owner = %s, want %s", name, dbtest.Outcome(err), stateRestrict)
		}
		for _, personal := range []string{markerName, markerMail, markerPhone, "ContractNotice", "changed"} {
			if strings.Contains(err.Error(), personal) {
				t.Errorf("the refusal of the %s repeats %q: %v", name, personal, err)
			}
		}
	}
	if !strings.Contains(update.Error(), "anbud_redact_raw_document()") {
		t.Errorf("the refusal of an UPDATE does not name the function that erases: %v", update)
	}
	if strings.Contains(remove.Error(), "anbud_redact_raw_document") {
		t.Errorf("the refusal of a DELETE names the function, which cannot delete a row either: %v", remove)
	}
}

// Setting the door by hand gives no privilege. A role that cannot UPDATE or DELETE on
// raw_documents still cannot, whatever it sets, and the refusal is the server's.
func TestSettingTheDoorByHandGivesNoPrivilege(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	a := env.seedNotice(ctx, t, "2026-100001", contactBody(""))

	for name, pool := range map[string]*pgxpool.Pool{"anbud_app": env.app, "a member of anbud_dba": env.dba} {
		tx, err := pool.Begin(ctx)
		if err != nil {
			t.Fatalf("begin as %s: %s", name, dbtest.Describe(err))
		}
		xid := scanInt(ctx, t, tx, `SELECT txid_current()`)
		for what, stmt := range map[string]step{
			"the redaction update": narrow(a),
			"another UPDATE":       do(`UPDATE raw_documents SET content_type = 'changed' WHERE id = $1`, a),
			"a DELETE":             do(`DELETE FROM raw_documents WHERE id = $1`, a),
		} {
			if got := try(ctx, t, tx, door(a, xid), stmt); got != stateDenied {
				t.Errorf("%s as %s with the door set by hand = %s, want %s from the privileges", what, name, got, stateDenied)
			}
		}
		_ = tx.Rollback(ctx)
	}
	if after := readDoc(ctx, t, env.admin, a); after.RedactedAt != nil || len(after.Body) == 0 {
		t.Fatal("a role without UPDATE changed the row")
	}
}

// A door set for the session, which survives a COMMIT, is of no use in the next
// transaction: it names the transaction it was set for.
func TestADoorSetForTheSessionDoesNotOutliveItsTransaction(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	a := env.seedNotice(ctx, t, "2026-100001", contactBody(""))

	conn, err := env.owner.Acquire(ctx)
	if err != nil {
		t.Fatalf("acquire as the owner: %s", dbtest.Describe(err))
	}
	defer func() {
		_, _ = conn.Exec(context.Background(), "DISCARD ALL")
		conn.Release()
	}()
	if _, err := conn.Exec(ctx, tombstoneByHandSQL, a, nil, 0, 0); err != nil {
		t.Fatalf("write a tombstone by hand as the owner: %s", dbtest.Describe(err))
	}
	earlier := scanInt(ctx, t, conn, `SELECT txid_current()`) // an autocommitted transaction of its own
	if _, err := conn.Exec(ctx, fmt.Sprintf(`SET %s = '%d:%d'`, doorSetting, a, earlier)); err != nil {
		t.Fatalf("set the door for the session: %s", dbtest.Describe(err))
	}

	tx, err := conn.Begin(ctx)
	if err != nil {
		t.Fatalf("begin: %s", dbtest.Describe(err))
	}
	defer func() { _ = tx.Rollback(ctx) }()
	// The tombstone was written in an earlier transaction, so the update takes its time
	// and is right in every way but the door.
	redactAtTheTombstoneTime := do(`UPDATE raw_documents SET body = ''::bytea, redacted_at = (SELECT t.redacted_at FROM raw_redactions t WHERE t.raw_document_id = $1) WHERE id = $1`, a)
	if got := try(ctx, t, tx, redactAtTheTombstoneTime); got != stateRestrict {
		t.Errorf("the redaction update under a session door of an earlier transaction = %s, want %s", got, stateRestrict)
	}
	xid := scanInt(ctx, t, tx, `SELECT txid_current()`)
	if got := try(ctx, t, tx, door(a, xid), redactAtTheTombstoneTime); got != stateOK {
		t.Errorf("control: the same update with the door of this transaction = %s, want ok", got)
	}
}

// The tombstone table is append-only, holds one record per document, checks what it
// holds, and has no place for a body: its columns are pinned, so a new one fails here
// until someone has looked at whether it can hold personal data.
func TestRedactionsTableIsAppendOnlyOnePerDocumentAndHoldsNoBody(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	redacted := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	live := env.seedNotice(ctx, t, "2026-100002", contactBody("<Live/>"))
	var tomb int64
	if err := env.dba.QueryRow(ctx, redactSQL, redacted, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("redact: %s", dbtest.Describe(err))
	}

	columns := scanStrings(ctx, t, env.admin, `SELECT column_name::text FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'raw_redactions' ORDER BY ordinal_position`)
	want := []string{"id", "raw_document_id", "doffin_id", "original_sha256", "original_size", "reason", "request_ref", "redacted_by", "redacted_at"}
	if !slices.Equal(columns, want) {
		t.Errorf("raw_redactions columns = %v, want %v", columns, want)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM information_schema.columns WHERE table_schema = 'public' AND table_name = 'raw_redactions' AND data_type = 'bytea'`); got != 0 {
		t.Errorf("raw_redactions has %d bytea column(s); a tombstone holds no bytes of a notice", got)
	}

	tx, err := env.owner.Begin(ctx)
	if err != nil {
		t.Fatalf("begin as the owner: %s", dbtest.Describe(err))
	}
	defer func() { _ = tx.Rollback(ctx) }()
	for name, stmt := range map[string]string{
		"UPDATE":   `UPDATE raw_redactions SET reason = 'objection'`,
		"DELETE":   `DELETE FROM raw_redactions`,
		"TRUNCATE": `TRUNCATE raw_redactions`,
	} {
		if got := try(ctx, t, tx, do(stmt)); got != stateRestrict {
			t.Errorf("%s on raw_redactions by the owner = %s, want %s", name, got, stateRestrict)
		}
	}

	const insert = `INSERT INTO raw_redactions (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by) VALUES ($1, $2, $3, $4, $5, $6, $7)`
	valid := []any{live, "2026-100002", strings.Repeat("a", 64), 10, "objection", "T-1", "owner"}
	with := func(i int, v any) []any {
		args := slices.Clone(valid)
		args[i] = v
		return args
	}
	for _, c := range []struct {
		name string
		args []any
		want string
	}{
		{"control: a valid record for a row that has none", valid, stateOK},
		{"a reason that is not on the list", with(4, "because"), stateCheck},
		{"a request reference with a space", with(5, "T 1"), stateCheck},
		{"a request reference of 121 characters", with(5, strings.Repeat("a", 121)), stateCheck},
		{"an upper-case hash", with(2, strings.Repeat("A", 64)), stateCheck},
		{"a size of zero", with(3, 0), stateCheck},
		{"a notice id of the wrong shape", with(1, "2026-1"), stateCheck},
		{"nobody as the caller", with(6, ""), stateCheck},
		{"a second record for a row that has one", with(0, redacted), stateUnique},
		{"a row that does not exist", with(0, int64(999999)), stateNoReference},
	} {
		if got := try(ctx, t, tx, do(insert, c.args...)); got != c.want {
			t.Errorf("%s = %s, want %s", c.name, got, c.want)
		}
	}
}

// execer is what the pools share.
type execer interface {
	Exec(ctx context.Context, sql string, args ...any) (pgconn.CommandTag, error)
}

// A new row cannot arrive already redacted, whoever writes it, by whatever route: that
// would store a row the relaxed checks do not look at. An ordinary row is still held to
// the checks, with its hash and its body.
func TestANewRowCannotArriveRedacted(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)

	const (
		ordinary  = `INSERT INTO raw_documents (source, doffin_id, body, body_sha256, fetched_at, run_id) VALUES ('doffin', $1, $2::bytea, encode(sha256($2::bytea), 'hex'), now(), $3)`
		redacted  = `INSERT INTO raw_documents (source, doffin_id, body, body_sha256, fetched_at, run_id, redacted_at) VALUES ('doffin', $1, $2::bytea, encode(sha256($2::bytea), 'hex'), now(), $3, now())`
		unchecked = `INSERT INTO raw_documents (source, doffin_id, body, body_sha256, fetched_at, run_id, redacted_at) VALUES ('doffin', $1, ''::bytea, repeat('a', 64), now(), $2, now())`
	)
	body := []byte("a notice body")
	stored := 0
	for _, writer := range []struct {
		name string
		pool execer
		base int
	}{{"anbud_app", env.app, 200100}, {"the owner", env.owner, 200200}} {
		name, pool := writer.name, writer.pool
		id := func(n int) string { return fmt.Sprintf("2026-%06d", writer.base+n) }
		for _, c := range []struct {
			what string
			sql  string
			args []any
			want string
		}{
			{"control: an ordinary row", ordinary, []any{id(1), body, env.run}, stateOK},
			{"a row that carries redacted_at", redacted, []any{id(2), body, env.run}, stateRestrict},
			{"an empty row with a made-up hash that carries redacted_at", unchecked, []any{id(3), env.run}, stateRestrict},
			{"a duplicate that carries redacted_at, with ON CONFLICT DO NOTHING", redacted + ` ON CONFLICT (source, doffin_id, body_sha256) DO NOTHING`, []any{id(1), body, env.run}, stateRestrict},
			{"an ordinary row with a hash that is not the body's", `INSERT INTO raw_documents (source, doffin_id, body, body_sha256, fetched_at, run_id) VALUES ('doffin', $1, $2::bytea, repeat('a', 64), now(), $3)`, []any{id(4), body, env.run}, stateCheck},
			{"an ordinary row with an empty body", `INSERT INTO raw_documents (source, doffin_id, body, body_sha256, fetched_at, run_id) VALUES ('doffin', $1, ''::bytea, encode(sha256(''::bytea), 'hex'), now(), $2)`, []any{id(5), env.run}, stateCheck},
		} {
			_, err := pool.Exec(ctx, c.sql, c.args...)
			got := dbtest.Outcome(err)
			if got != c.want {
				t.Errorf("%s as %s = %s (%s), want %s", c.what, name, got, dbtest.Describe(err), c.want)
			}
			if got == stateOK {
				stored++
			}
		}
	}
	// Through the view, which the owner may write to as to any table: the base table's
	// guard still fires.
	if _, err := env.owner.Exec(ctx, `INSERT INTO raw_documents_live (source, doffin_id, body, body_sha256, fetched_at, run_id, redacted_at) VALUES ('doffin', '2026-299999', $1::bytea, encode(sha256($1::bytea), 'hex'), now(), $2, now())`, body, env.run); dbtest.Outcome(err) != stateRestrict {
		t.Errorf("a row that carries redacted_at, inserted through raw_documents_live = %s, want %s", dbtest.Outcome(err), stateRestrict)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents`); got != int64(stored) {
		t.Errorf("raw_documents has %d rows, want the %d ordinary ones: a refused insert left a row", got, stored)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents WHERE redacted_at IS NOT NULL`); got != 0 {
		t.Errorf("%d rows arrived redacted", got)
	}
}
