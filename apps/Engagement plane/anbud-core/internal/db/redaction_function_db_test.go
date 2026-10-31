package db_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The redaction function, end to end, as a member of anbud_dba calls it. Needs a throwaway
// server: run with make test-db.

// facts is every column of the row except its body, so that a failure message can say
// what changed without printing personal data.
func (r docRow) facts() string {
	return fmt.Sprintf("id=%d source=%s doffin_id=%s data_class=%s content_type=%q sha256=%s fetched=%s stored=%s run=%d",
		r.ID, r.Source, r.DoffinID, r.DataClass, r.ContentType, r.BodySHA256,
		r.FetchedAt.UTC().Format(time.RFC3339Nano), r.StoredAt.UTC().Format(time.RFC3339Nano), r.RunID)
}

// One call erases one body, in place, and writes the tombstone: the row stays and keeps
// every other column, the hash of the original bytes included. Nothing else is touched.
func TestRedactionErasesTheBodyAndRecordsATombstone(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	original := contactBody("")
	target := env.seedNotice(ctx, t, "2026-100001", original)
	other := env.seedNotice(ctx, t, "2026-100002", contactBody("<Other/>"))
	before, otherBefore := readDoc(ctx, t, env.admin, target), readDoc(ctx, t, env.admin, other)
	sum := sha256.Sum256(original)
	wantHash := hex.EncodeToString(sum[:])
	if !bytes.Contains(before.Body, []byte(markerMail)) || before.BodySHA256 != wantHash {
		t.Fatal("the seeded notice does not hold the contact details the test erases")
	}

	var tomb int64
	if err := env.dba.QueryRow(ctx, redactSQL, target, "erasure_request", "TICKET-2026/17").Scan(&tomb); err != nil {
		t.Fatalf("redact as a member of anbud_dba: %s", dbtest.Describe(err))
	}

	after := readDoc(ctx, t, env.admin, target)
	if len(after.Body) != 0 || scanInt(ctx, t, env.admin, `SELECT octet_length(body) FROM raw_documents WHERE id = $1`, target) != 0 {
		t.Error("the body is not empty after the redaction")
	}
	if after.RedactedAt == nil {
		t.Fatal("redacted_at is not set after the redaction")
	}
	if after.BodySHA256 != wantHash {
		t.Errorf("body_sha256 = %s, want the hash of the original bytes, %s", after.BodySHA256, wantHash)
	}
	if after.facts() != before.facts() {
		t.Errorf("a column other than body and redacted_at changed:\n before %s\n after  %s", before.facts(), after.facts())
	}
	if otherAfter := readDoc(ctx, t, env.admin, other); !bytes.Equal(otherAfter.Body, otherBefore.Body) || otherAfter.RedactedAt != nil || otherAfter.facts() != otherBefore.facts() {
		t.Error("the redaction changed another notice")
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents`); got != 2 {
		t.Errorf("raw_documents has %d rows after the redaction, want the 2 it had", got)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM download_attempts WHERE raw_document_id = $1`, target); got != 1 {
		t.Errorf("the notice's download attempt points at it %d times, want 1: the row must stay", got)
	}

	var tb struct {
		ID, DocID, Size                    int64
		DoffinID, SHA, Reason, Request, By string
		At                                 time.Time
	}
	if err := env.admin.QueryRow(ctx, `SELECT id, raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by, redacted_at FROM raw_redactions`).
		Scan(&tb.ID, &tb.DocID, &tb.DoffinID, &tb.SHA, &tb.Size, &tb.Reason, &tb.Request, &tb.By, &tb.At); err != nil {
		t.Fatalf("read the tombstone: %s", dbtest.Describe(err))
	}
	if tb.ID != tomb || tb.DocID != target || tb.DoffinID != "2026-100001" || tb.SHA != wantHash || tb.Size != int64(len(original)) ||
		tb.Reason != "erasure_request" || tb.Request != "TICKET-2026/17" || tb.By != dbaLogin || !tb.At.Equal(*after.RedactedAt) {
		t.Errorf("tombstone = %+v (function returned %d), want the document, its original hash and size, the reason, the reference, the caller %s and the time of the redaction", tb, tomb, dbaLogin)
	}
	var tombstoneText string
	if err := env.admin.QueryRow(ctx, `SELECT string_agg(t::text, ' ') FROM raw_redactions t`).Scan(&tombstoneText); err != nil {
		t.Fatalf("read the tombstones as text: %s", dbtest.Describe(err))
	}
	for _, personal := range []string{markerName, "Nordmann", markerMail, markerPhone, "<ContractNotice"} {
		if strings.Contains(tombstoneText, personal) {
			t.Errorf("the tombstone carries %q", personal)
		}
	}

	// The parser's contract: the view hides the redacted row and shows the other one.
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents_live WHERE id = $1`, target); got != 0 {
		t.Errorf("raw_documents_live shows the redacted row (%d)", got)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents_live WHERE id = $1`, other); got != 1 {
		t.Errorf("raw_documents_live shows the other row %d times, want once", got)
	}

	// The identity survives, so downloading the same bytes again stores nothing.
	tag, err := env.app.Exec(ctx, `INSERT INTO raw_documents (source, doffin_id, content_type, body, body_sha256, fetched_at, run_id) VALUES ('doffin', $1, 'application/xml', $2::bytea, encode(sha256($2::bytea), 'hex'), now(), $3) ON CONFLICT (source, doffin_id, body_sha256) DO NOTHING`,
		"2026-100001", original, env.run)
	if err != nil || tag.RowsAffected() != 0 {
		t.Errorf("storing the erased bytes again = %d rows, error %s; want none, as a duplicate", tag.RowsAffected(), dbtest.Describe(err))
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents`); got != 2 {
		t.Errorf("raw_documents has %d rows after the same bytes arrived again, want 2", got)
	}
}

// A call that is refused writes nothing and says nothing about what it was given: a
// request reference typed with a name in it must not come back in the error, which may
// reach a log.
func TestRedactionRefusesWhatItShouldAndWritesNothing(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	id := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	before := readDoc(ctx, t, env.admin, id)

	const typedWithAName = "Kari Nordmann PERSONAL-MARKER"
	cases := []struct {
		name    string
		id      any
		reason  any
		request any
		want    string
	}{
		{"an id no row has", int64(999999), "erasure_request", "T-1", stateNoSuchDocument},
		{"no id", nil, "erasure_request", "T-1", stateNullArg},
		{"no reason", id, nil, "T-1", stateNullArg},
		{"no request", id, "erasure_request", nil, stateNullArg},
		{"a reason that is not on the list", id, "because", "T-1", stateBadArg},
		{"a reason in capitals", id, "ERASURE_REQUEST", "T-1", stateBadArg},
		{"a reason with a trailing space", id, "erasure_request ", "T-1", stateBadArg},
		{"an empty reason", id, "", "T-1", stateBadArg},
		{"an empty request", id, "erasure_request", "", stateBadArg},
		{"a request typed with a name", id, "erasure_request", typedWithAName, stateBadArg},
		{"a request that is an e-mail address", id, "erasure_request", markerMail, stateBadArg},
		{"a request with a trailing newline", id, "erasure_request", "T-1\n", stateBadArg},
		{"a request of 121 characters", id, "erasure_request", strings.Repeat("a", 121), stateBadArg},
		{"a request with a letter outside ASCII", id, "erasure_request", "T-æ", stateBadArg},
	}
	for _, c := range cases {
		var tomb int64
		err := env.dba.QueryRow(ctx, redactSQL, c.id, c.reason, c.request).Scan(&tomb)
		if got := dbtest.Outcome(err); got != c.want {
			t.Errorf("%s: outcome %s (%s), want %s", c.name, got, dbtest.Describe(err), c.want)
			continue
		}
		for _, given := range []any{c.reason, c.request} {
			if s, ok := given.(string); ok && len(s) > 3 && strings.Contains(err.Error(), s) {
				t.Errorf("%s: the error repeats what it was given: %q", c.name, err.Error())
			}
		}
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 0 {
		t.Errorf("refused calls left %d tombstones, want none", got)
	}
	if after := readDoc(ctx, t, env.admin, id); !bytes.Equal(after.Body, before.Body) || after.RedactedAt != nil {
		t.Fatal("a refused call changed the row")
	}

	// A second call on the same id is refused, by the DBA and by the owner alike, and the
	// first redaction stands.
	var first int64
	if err := env.dba.QueryRow(ctx, redactSQL, id, "objection", "T-2").Scan(&first); err != nil {
		t.Fatalf("the first redaction: %s", dbtest.Describe(err))
	}
	done := readDoc(ctx, t, env.admin, id)
	for name, pool := range map[string]queryRower{"a member of anbud_dba": env.dba, "the owner": env.owner} {
		var again int64
		err := pool.QueryRow(ctx, redactSQL, id, "controller_decision", "T-3").Scan(&again)
		if got := dbtest.Outcome(err); got != stateAlreadyRedacted {
			t.Errorf("a second redaction by %s = %s, want %s", name, got, stateAlreadyRedacted)
		}
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 1 {
		t.Errorf("tombstones = %d after the second calls, want the one", got)
	}
	var reason string
	if err := env.admin.QueryRow(ctx, `SELECT reason FROM raw_redactions WHERE id = $1`, first).Scan(&reason); err != nil || reason != "objection" {
		t.Errorf("the first tombstone's reason = %q (error %s), want objection", reason, dbtest.Describe(err))
	}
	if after := readDoc(ctx, t, env.admin, id); after.RedactedAt == nil || !after.RedactedAt.Equal(*done.RedactedAt) {
		t.Error("a second call changed redacted_at")
	}
}

// One transaction: when the update of the row fails after the tombstone is written, the
// tombstone goes with it, and the row is as it was. A second trigger that fails stands in
// for any late failure.
func TestRedactionIsOneTransaction(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	id := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	before := readDoc(ctx, t, env.admin, id)

	for _, stmt := range []string{
		`CREATE FUNCTION zz_inject_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected failure after the guard'; END $$`,
		`CREATE TRIGGER zz_inject_failure BEFORE UPDATE ON raw_documents FOR EACH ROW EXECUTE FUNCTION zz_inject_failure()`,
	} {
		if _, err := env.admin.Exec(ctx, stmt); err != nil {
			t.Fatalf("arrange the injected failure: %s", dbtest.Describe(err))
		}
	}
	var tomb int64
	err := env.dba.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb)
	if got := dbtest.Outcome(err); got != stateRaised || !strings.Contains(err.Error(), "injected failure") {
		t.Fatalf("the redaction with a failing trigger = %s (%s), want the injected failure %s", got, dbtest.Describe(err), stateRaised)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 0 {
		t.Errorf("a failed redaction left %d tombstone(s)", got)
	}
	if after := readDoc(ctx, t, env.admin, id); !bytes.Equal(after.Body, before.Body) || after.RedactedAt != nil {
		t.Error("a failed redaction changed the row")
	}

	// With the failure gone the same call succeeds: nothing from the failed try is left.
	if _, err := env.admin.Exec(ctx, `DROP TRIGGER zz_inject_failure ON raw_documents`); err != nil {
		t.Fatalf("remove the injected failure: %s", dbtest.Describe(err))
	}
	if err := env.dba.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Errorf("the redaction after the failure was removed: %s", dbtest.Describe(err))
	}
}

// The function locks the row: a second caller that arrives while the first transaction is
// still open waits for it, and once the first has committed is refused as late ("already
// redacted"), not with a clash on the tombstone's unique key, which is what a caller
// that did not wait for the lock would meet.
func TestRedactionLocksTheRowSoASecondCallerWaitsAndIsRefused(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	id := env.seedNotice(ctx, t, "2026-100001", contactBody(""))

	first, err := env.dba.Begin(ctx)
	if err != nil {
		t.Fatalf("begin the first transaction: %s", dbtest.Describe(err))
	}
	defer func() { _ = first.Rollback(ctx) }()
	var tomb int64
	if err := first.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("redact in the first transaction: %s", dbtest.Describe(err))
	}

	type outcome struct {
		code string
		err  error
	}
	done := make(chan outcome, 1)
	go func() {
		var other int64
		err := env.dba.QueryRow(ctx, redactSQL, id, "objection", "T-2").Scan(&other)
		done <- outcome{dbtest.Outcome(err), err}
	}()

	// The second call is waiting on a lock, and has not finished.
	deadline := time.Now().Add(20 * time.Second)
	for waiting := false; !waiting; {
		select {
		case o := <-done:
			t.Fatalf("the second call finished (%s) while the first transaction was open: the row was not locked", o.code)
		default:
		}
		waiting = scanInt(ctx, t, env.admin, `SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND wait_event_type = 'Lock' AND query LIKE '%anbud_redact_raw_document%'`) > 0
		if !waiting {
			if time.Now().After(deadline) {
				t.Fatal("the second call never waited on a lock")
			}
			time.Sleep(50 * time.Millisecond)
		}
	}

	if err := first.Commit(ctx); err != nil {
		t.Fatalf("commit the first transaction: %s", dbtest.Describe(err))
	}
	select {
	case o := <-done:
		if o.code != stateAlreadyRedacted {
			t.Errorf("the second call = %s (%s), want %s: it should find the row redacted once it gets the lock", o.code, dbtest.Describe(o.err), stateAlreadyRedacted)
		}
	case <-time.After(20 * time.Second):
		t.Fatal("the second call did not finish after the first transaction committed")
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 1 {
		t.Errorf("tombstones = %d, want the first call's alone", got)
	}
	var reason string
	if err := env.admin.QueryRow(ctx, `SELECT reason FROM raw_redactions`).Scan(&reason); err != nil || reason != "erasure_request" {
		t.Errorf("the surviving tombstone's reason = %q (error %s), want the first call's, erasure_request", reason, dbtest.Describe(err))
	}
}

// The function shuts the door behind itself: in the same transaction, a statement that
// follows the redaction finds no door open.
func TestRedactionClosesTheDoorBehindItself(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	id := env.seedNotice(ctx, t, "2026-100001", contactBody(""))

	tx, err := env.owner.Begin(ctx)
	if err != nil {
		t.Fatalf("begin as the owner: %s", dbtest.Describe(err))
	}
	defer func() { _ = tx.Rollback(ctx) }()
	var tomb int64
	if err := tx.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("redact: %s", dbtest.Describe(err))
	}
	var open *string
	if err := tx.QueryRow(ctx, `SELECT current_setting($1, true)`, doorSetting).Scan(&open); err != nil {
		t.Fatalf("read the door: %s", dbtest.Describe(err))
	}
	if open != nil && *open != "" {
		t.Errorf("the door is still open after the redaction: %q", *open)
	}
}

// The server log gets one line per redaction, with ids and the reason. It holds nothing
// of the body, and not the request reference either, which may carry a name by mistake.
func TestRedactionLogLineCarriesIdsOnly(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	id := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	conn, received := connectLogging(ctx, t, env.dbaDSN)

	var tomb int64
	if err := conn.QueryRow(ctx, redactSQL, id, "objection", "TICKET-9").Scan(&tomb); err != nil {
		t.Fatalf("redact: %s", dbtest.Describe(err))
	}

	var lines []string
	for _, m := range received() {
		if strings.HasPrefix(m, "LOG: ") && strings.Contains(m, "redaction") {
			lines = append(lines, m)
		}
	}
	if len(lines) != 1 {
		t.Fatalf("the session received %d redaction log lines, want one: %q", len(lines), received())
	}
	for _, want := range []string{fmt.Sprint(id), fmt.Sprint(tomb), "2026-100001", "objection"} {
		if !strings.Contains(lines[0], want) {
			t.Errorf("the log line %q lacks %q", lines[0], want)
		}
	}
	for _, banned := range []string{markerName, "Nordmann", markerMail, markerPhone, "ContractNotice", "TICKET-9"} {
		if strings.Contains(lines[0], banned) {
			t.Errorf("the log line carries %q: %q", banned, lines[0])
		}
	}
}

// Every role may create a temporary table, and a temporary table is looked up before the
// schemas of a search path unless the path names pg_temp. A member of anbud_dba who creates
// temporary tables named like the tombstone table, the notice table and the view of the notices
// that are still held must not be able to make the function write its record, read a notice
// or erase a body in them. Each name has a control: a function written loosely, which names its
// table with no schema and pins no search path, does what the attacker wants with it. The view
// cannot matter to the real function, which never names it; the control shows what the name
// could do to a function that did.
func TestRedactionCannotBeHijackedThroughTemporaryTables(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	target := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	control := env.seedNotice(ctx, t, "2026-100002", contactBody("<Control/>"))
	targetBefore, controlBefore := readDoc(ctx, t, env.admin, target), readDoc(ctx, t, env.admin, control)

	// Four loose functions, each running as the server's role and each naming its table the way
	// a careless function would: a tombstone insert, a reader of a notice's id, an eraser and a counter.
	for _, stmt := range []string{
		`CREATE FUNCTION public.anbud_test_loose_tombstone(p_id bigint) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER AS $$
		 DECLARE v bigint;
		 BEGIN
		     INSERT INTO raw_redactions (raw_document_id, doffin_id, original_sha256, original_size, reason, request_ref, redacted_by)
		     SELECT d.id, d.doffin_id, d.body_sha256, octet_length(d.body), 'controller_decision', 'CONTROL-1', 'control'
		       FROM raw_documents d WHERE d.id = p_id
		     RETURNING id INTO v;
		     RETURN v;
		 END $$`,
		`CREATE FUNCTION public.anbud_test_loose_doffin_id(p_id bigint) RETURNS text LANGUAGE plpgsql SECURITY DEFINER AS $$
		 DECLARE v text;
		 BEGIN
		     SELECT d.doffin_id INTO v FROM raw_documents d WHERE d.id = p_id;
		     RETURN v;
		 END $$`,
		`CREATE FUNCTION public.anbud_test_loose_erase(p_id bigint) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER AS $$
		 DECLARE v bigint;
		 BEGIN
		     UPDATE raw_documents SET body = ''::bytea, redacted_at = now() WHERE id = p_id;
		     GET DIAGNOSTICS v = ROW_COUNT;
		     RETURN v;
		 END $$`,
		`CREATE FUNCTION public.anbud_test_loose_count(p_table text) RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER AS $$
		 DECLARE v bigint;
		 BEGIN
		     EXECUTE 'SELECT count(*) FROM ' || p_table INTO v;
		     RETURN v;
		 END $$`,
		`GRANT EXECUTE ON FUNCTION public.anbud_test_loose_tombstone(bigint), public.anbud_test_loose_doffin_id(bigint), public.anbud_test_loose_erase(bigint), public.anbud_test_loose_count(text) TO anbud_dba`,
	} {
		if _, err := env.admin.Exec(ctx, stmt); err != nil {
			t.Fatalf("arrange the control functions: %s", dbtest.Describe(err))
		}
	}

	conn, err := env.dba.Acquire(ctx)
	if err != nil {
		t.Fatalf("acquire a connection as a member of anbud_dba: %s", dbtest.Describe(err))
	}
	defer func() {
		_, _ = conn.Exec(context.Background(), "DISCARD ALL")
		conn.Release()
	}()
	run := func(stmt string, args ...any) {
		t.Helper()
		if _, err := conn.Exec(ctx, stmt, args...); err != nil {
			t.Fatalf("a member of anbud_dba cannot run %q, so the test attacks nothing: %s", stmt, dbtest.Describe(err))
		}
	}

	// The three temporary tables, and a forged copy of each notice in the one named like the
	// notice table: another identifier (2999- and the row's id) and another body, with a hash that
	// matches the forged body.
	forgedID := func(id int64) string { return fmt.Sprintf("2999-%06d", id) }
	run(`CREATE TEMP TABLE raw_redactions (LIKE public.raw_redactions INCLUDING ALL)`)
	run(`CREATE TEMP TABLE raw_documents (LIKE public.raw_documents INCLUDING ALL)`)
	run(`CREATE TEMP TABLE raw_documents_live (LIKE public.raw_documents_live)`)
	for _, id := range []int64{target, control} {
		run(`INSERT INTO pg_temp.raw_documents (id, source, doffin_id, content_type, body, body_sha256, fetched_at, run_id)
		     OVERRIDING SYSTEM VALUE
		     SELECT id, source, '2999-' || lpad(id::text, 6, '0'), content_type, 'forged'::bytea, encode(sha256('forged'::bytea), 'hex'), fetched_at, run_id
		       FROM public.raw_documents WHERE id = $1`, id)
	}
	forgedSum := sha256.Sum256([]byte("forged"))
	forgedHash := hex.EncodeToString(forgedSum[:])

	// Control 1, the tombstone table: the loose function writes its tombstone into the temporary
	// table, and the real table gets none. Its row is made from the forged notice it read.
	var loose int64
	if err := conn.QueryRow(ctx, `SELECT public.anbud_test_loose_tombstone($1)`, control).Scan(&loose); err != nil {
		t.Fatalf("the control function: %s", dbtest.Describe(err))
	}
	if inTemp, inReal := scanInt(ctx, t, conn, `SELECT count(*) FROM pg_temp.raw_redactions`), scanInt(ctx, t, env.admin, `SELECT count(*) FROM public.raw_redactions`); inTemp != 1 || inReal != 0 {
		t.Fatalf("control: the loose function wrote %d row(s) to the temporary table and %d to the real one, want 1 and 0: the attack does not work here, so the test below proves nothing", inTemp, inReal)
	}

	// Control 2, the notice table, read: the loose function returns the identifier of the forged
	// notice, and not the real one.
	var seen string
	if err := conn.QueryRow(ctx, `SELECT public.anbud_test_loose_doffin_id($1)`, control).Scan(&seen); err != nil || seen != forgedID(control) {
		t.Fatalf("control: the loose reader returned %q (error %s), want the forged identifier %s: the attack does not work here", seen, dbtest.Describe(err), forgedID(control))
	}
	if controlBefore.DoffinID == forgedID(control) {
		t.Fatal("the real notice carries the forged identifier")
	}

	// Control 3, the notice table, written: the loose eraser erases the forged copy and reports
	// one row, and the real notice keeps its body.
	var erased int64
	if err := conn.QueryRow(ctx, `SELECT public.anbud_test_loose_erase($1)`, control).Scan(&erased); err != nil || erased != 1 {
		t.Fatalf("control: the loose eraser changed %d rows (error %s), want 1, the forged copy", erased, dbtest.Describe(err))
	}
	if doc := readDoc(ctx, t, env.admin, control); !bytes.Equal(doc.Body, controlBefore.Body) || doc.RedactedAt != nil {
		t.Fatal("control: the loose eraser touched the real notice, so it was not redirected")
	}

	// Control 4, the view: the loose counter counts the empty temporary table, not the notices that
	// are still held.
	var live int64
	if err := conn.QueryRow(ctx, `SELECT public.anbud_test_loose_count('raw_documents_live')`).Scan(&live); err != nil || live != 0 {
		t.Fatalf("control: the loose counter counted %d rows (error %s), want 0 from the temporary table", live, dbtest.Describe(err))
	}
	if real := scanInt(ctx, t, env.admin, `SELECT count(*) FROM public.raw_documents_live`); real != 2 {
		t.Fatalf("the notices still held are %d, want 2", real)
	}

	// The attack on the real function, with all three temporary tables in place.
	tombstonesInTemp := scanInt(ctx, t, conn, `SELECT count(*) FROM pg_temp.raw_redactions`)
	var tomb int64
	if err := conn.QueryRow(ctx, redactSQL, target, "erasure_request", "TICKET-HIJACK").Scan(&tomb); err != nil {
		t.Fatalf("the redaction with temporary tables in place: %s", dbtest.Describe(err))
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM public.raw_redactions WHERE raw_document_id = $1`, target); got != 1 {
		t.Errorf("the real tombstone table holds %d records for the notice, want 1: the record was lost to a temporary table", got)
	}
	if got := scanInt(ctx, t, conn, `SELECT count(*) FROM pg_temp.raw_redactions`); got != tombstonesInTemp {
		t.Errorf("the temporary tombstone table holds %d records, had %d: it received a record of the redaction", got, tombstonesInTemp)
	}
	// The tombstone is made from the real notice and not from the forged copy.
	var recordedID, recordedHash string
	if err := env.admin.QueryRow(ctx, `SELECT doffin_id, original_sha256 FROM public.raw_redactions WHERE raw_document_id = $1`, target).Scan(&recordedID, &recordedHash); err != nil {
		t.Fatalf("read the tombstone: %s", dbtest.Describe(err))
	}
	if recordedID != targetBefore.DoffinID || recordedHash != targetBefore.BodySHA256 || recordedHash == forgedHash || recordedID == forgedID(target) {
		t.Errorf("the tombstone records notice %q with hash %q, want the real notice %q with hash %q, and nothing from the forged copy", recordedID, recordedHash, targetBefore.DoffinID, targetBefore.BodySHA256)
	}
	if got := scanInt(ctx, t, conn, `SELECT octet_length(body) FROM pg_temp.raw_documents WHERE id = $1`, target); got != int64(len("forged")) {
		t.Errorf("the forged copy has a body of %d bytes after the redaction, want the %d it had: the erasure was redirected to it", got, len("forged"))
	}
	if got := scanInt(ctx, t, conn, `SELECT count(*) FROM pg_temp.raw_documents_live`); got != 0 {
		t.Errorf("the temporary table named like the view holds %d rows", got)
	}
	if after := readDoc(ctx, t, env.admin, target); len(after.Body) != 0 || after.RedactedAt == nil {
		t.Error("the real row was not redacted")
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM public.raw_documents_live`); got != 1 {
		t.Errorf("the notices still held are %d after the redaction, want the 1 that was not redacted", got)
	}
}

// If anything makes the erasing UPDATE change no row, the function must fail and take its
// tombstone back with it. It must not hand out a tombstone for a body that is still there. A
// trigger that swallows the update is one way to make it happen, here added by the owner
// and firing after the guard, which has already let the update through.
func TestRedactionFailsClosedWhenTheUpdateChangesNoRow(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	target := env.seedNotice(ctx, t, "2026-100001", contactBody(""))

	for _, sql := range []string{
		`CREATE FUNCTION zz_swallow() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$`,
		`CREATE TRIGGER zz_swallow BEFORE UPDATE ON raw_documents FOR EACH ROW EXECUTE FUNCTION zz_swallow()`,
	} {
		if _, err := env.owner.Exec(ctx, sql); err != nil {
			t.Fatalf("arrange %q as the owner: %s", sql, dbtest.Describe(err))
		}
	}

	_, err := env.dba.Exec(ctx, redactSQL, target, "erasure_request", "T-1")
	if got := dbtest.SQLState(err); got != stateInternal {
		t.Errorf("a redaction whose UPDATE changed no row = %s, want the refusal %s", dbtest.Outcome(err), stateInternal)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 0 {
		t.Errorf("%d tombstones remain after a redaction that erased nothing, want none", got)
	}
	if doc := readDoc(ctx, t, env.admin, target); doc.RedactedAt != nil || len(doc.Body) == 0 {
		t.Error("the row was changed by a redaction that reported failure")
	}
}

// A caller that rolls its transaction back takes the whole redaction with it. The tombstone and
// the erasure are one transaction, so no tombstone is left for a body that is still there, the
// body is whole, and the door the function opened for its UPDATE is gone with the transaction.
// Inside the transaction the caller sees its own work, and no other session sees it before a commit.
func TestARedactionRolledBackByTheCallerLeavesNoTombstoneAndTheBodyIntact(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	id := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	before := readDoc(ctx, t, env.admin, id)

	tx, err := env.dba.Begin(ctx)
	if err != nil {
		t.Fatalf("begin as a member of anbud_dba: %s", dbtest.Describe(err))
	}
	defer func() { _ = tx.Rollback(ctx) }()
	var tomb int64
	if err := tx.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Fatalf("redact inside the transaction: %s", dbtest.Describe(err))
	}
	if got := scanInt(ctx, t, tx, `SELECT count(*) FROM raw_redactions`); got != 1 {
		t.Fatalf("the caller sees %d tombstones inside its transaction, want its own", got)
	}
	if got := scanInt(ctx, t, tx, `SELECT octet_length(body) FROM raw_documents WHERE id = $1`, id); got != 0 {
		t.Fatalf("the caller sees a body of %d bytes inside its transaction, want it erased", got)
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 0 {
		t.Errorf("another session sees %d tombstones before the commit", got)
	}
	if doc := readDoc(ctx, t, env.admin, id); !bytes.Equal(doc.Body, before.Body) || doc.RedactedAt != nil {
		t.Error("another session sees the erasure before the commit")
	}

	if err := tx.Rollback(ctx); err != nil {
		t.Fatalf("roll back: %s", dbtest.Describe(err))
	}

	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 0 {
		t.Errorf("a rolled-back redaction left %d tombstone(s)", got)
	}
	after := readDoc(ctx, t, env.admin, id)
	if !bytes.Equal(after.Body, before.Body) || after.RedactedAt != nil || after.facts() != before.facts() {
		t.Error("a rolled-back redaction changed the row")
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_documents_live WHERE id = $1`, id); got != 1 {
		t.Errorf("raw_documents_live shows the notice %d times after the rollback, want once: it is still held", got)
	}
	// The door is a setting of the transaction that is gone: nothing is left open for the next one.
	var open *string
	if err := env.dba.QueryRow(ctx, `SELECT current_setting($1, true)`, doorSetting).Scan(&open); err != nil {
		t.Fatalf("read the door: %s", dbtest.Describe(err))
	}
	if open != nil && *open != "" {
		t.Errorf("the door is open after the rollback: %q", *open)
	}
	// Nothing of the rolled-back attempt is left in the way of the real one.
	if err := env.dba.QueryRow(ctx, redactSQL, id, "erasure_request", "T-1").Scan(&tomb); err != nil {
		t.Errorf("the redaction after the rollback: %s", dbtest.Describe(err))
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 1 {
		t.Errorf("tombstones = %d after the redaction that was committed, want 1", got)
	}
}
