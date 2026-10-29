package db_test

import (
	"context"
	"regexp"
	"strconv"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The procedure of docs/PERSONAL-DATA.md, as a member of anbud_dba types it: every SQL
// block of the runbook, with its placeholders filled in, run in the order of the steps on
// one session. A block that does not run, or that answers otherwise than the runbook says
// it will, fails here and not on the day it is needed. Needs a throwaway server: run with
// make test-db.

const (
	runbookIdentifier = markerName
	runbookTicket     = "TICKET-RUNBOOK"
)

// runBlock fills in the placeholders of a runbook block, runs it on conn and returns the
// result of each statement.
func runBlock(ctx context.Context, t *testing.T, conn *pgx.Conn, blocks map[string]string, label string, fill map[string]string) []*pgconn.Result {
	t.Helper()
	sql, ok := blocks[label]
	if !ok {
		t.Fatalf("the runbook has no block %q", label)
	}
	for placeholder, value := range fill {
		sql = strings.ReplaceAll(sql, placeholder, value)
	}
	if left := regexp.MustCompile(`<[A-Z][A-Z0-9_]*>`).FindString(sql); left != "" {
		t.Fatalf("block %q still has the placeholder %s", label, left)
	}
	results, err := conn.PgConn().Exec(ctx, sql).ReadAll()
	if err != nil {
		t.Fatalf("runbook block %q failed: %s", label, dbtest.Describe(err))
	}
	return results
}

// cell returns the text of a result's cell, and fails t when there is none.
func cell(t *testing.T, r *pgconn.Result, row, col int) string {
	t.Helper()
	if row >= len(r.Rows) || col >= len(r.Rows[row]) {
		t.Fatalf("the result has %d rows, there is no cell (%d, %d)", len(r.Rows), row, col)
	}
	return string(r.Rows[row][col])
}

func TestRunbookSQLRuns(t *testing.T) {
	env := newRedactionEnv(t)
	ctx := roleCtx(t)
	blocks := runbookBlocks(t)

	// Two notices name the person, a third does not; a search page names the person too.
	named := env.seedNotice(ctx, t, "2026-100001", contactBody(""))
	alsoNamed := env.seedNotice(ctx, t, "2026-100002", contactBody("<Second/>"))
	other := env.seedNotice(ctx, t, "2026-100003", []byte("<ContractNotice><Name>Somebody Else</Name></ContractNotice>"))
	if _, err := env.app.Exec(ctx, seedPageSQL, env.run, []byte(`{"hits":[{"buyer":{"name":"`+markerName+` Enkeltpersonforetak"}}]}`)); err != nil {
		t.Fatalf("seed a search page that names the person: %s", dbtest.Describe(err))
	}
	original := readDoc(ctx, t, env.admin, named)

	dbtest.MustBeThrowaway(t, env.dbaDSN)
	cfg, err := pgx.ParseConfig(env.dbaDSN)
	if err != nil {
		t.Fatal("dbtest: refusing an unparsable DSN")
	}
	conn, err := pgx.ConnectConfig(ctx, cfg)
	if err != nil {
		t.Fatalf("connect as a member of anbud_dba: %s", dbtest.Describe(err))
	}
	t.Cleanup(func() { _ = conn.Close(context.Background()) })

	id := func(n int64) string { return strconv.FormatInt(n, 10) }
	fill := map[string]string{
		"<IDENTIFIER>": runbookIdentifier,
		"<REASON>":     "erasure_request",
		"<TICKET>":     runbookTicket,
	}

	// Step 0: the session is the DBA's own, and may run the function.
	who := runBlock(ctx, t, conn, blocks, "whoami", fill)
	if got := [3]string{cell(t, who[0], 0, 0), cell(t, who[0], 0, 1), cell(t, who[0], 0, 2)}; got != [3]string{dbaLogin, "t", "t"} {
		t.Errorf("whoami answered %v, want [%s t t]", got, dbaLogin)
	}
	// Three SHOWs: the statement log, the slow-statement threshold and the failing-statement
	// log, which writes the text of a refused call and so any identifier typed into it.
	if got := runBlock(ctx, t, conn, blocks, "logging", fill); len(got) != 3 {
		t.Errorf("the logging block returned %d results, want one for each of the 3 SHOWs", len(got))
	}

	// Step 1: the search finds the two notices that name the person, by id, and not the
	// third; the second search finds the page and no attempt.
	found := runBlock(ctx, t, conn, blocks, "find", fill)
	if len(found) != 1 || len(found[0].Rows) != 2 || cell(t, found[0], 0, 0) != id(named) || cell(t, found[0], 1, 0) != id(alsoNamed) {
		t.Errorf("the find block returned %d result(s) of %v rows, want the two notices %d and %d (and not %d)", len(found), rowCounts(found), named, alsoNamed, other)
	}
	if got := runBlock(ctx, t, conn, blocks, "find-other", fill); len(got) != 2 || len(got[0].Rows) != 1 || cell(t, got[1], 0, 0) != "0" {
		t.Errorf("the find-other block returned rows %v, want one search page and no attempt", rowCounts(got))
	}

	// Steps 2 and 3: redact inside a transaction, read the checks, commit.
	fill["<ID>"] = id(named)
	checks := runBlock(ctx, t, conn, blocks, "redact", fill)
	if len(checks) != 5 {
		t.Fatalf("the redact block returned %d results, want BEGIN and four SELECTs", len(checks))
	}
	tombstone := cell(t, checks[2], 0, 0)
	if got := [4]string{cell(t, checks[2], 0, 1), cell(t, checks[2], 0, 4), cell(t, checks[2], 0, 5), cell(t, checks[2], 0, 6)}; got != [4]string{id(named), "erasure_request", runbookTicket, dbaLogin} {
		t.Errorf("the tombstone the block shows = %v", got)
	}
	if bodyBytes, live := cell(t, checks[3], 0, 2), cell(t, checks[4], 0, 0); bodyBytes != "0" || live != "0" || cell(t, checks[3], 0, 3) == "" {
		t.Errorf("the checks show body_bytes %s, redacted_at %q, rows in the live view %s; want 0, a time, 0", bodyBytes, cell(t, checks[3], 0, 3), live)
	}
	runBlock(ctx, t, conn, blocks, "commit", fill)

	// What the checks said is what is stored, for another session too.
	after := readDoc(ctx, t, env.admin, named)
	if len(after.Body) != 0 || after.RedactedAt == nil || after.BodySHA256 != original.BodySHA256 {
		t.Error("after the runbook's redaction the row is not empty and redacted under its original hash")
	}
	var ref, by string
	if err := env.admin.QueryRow(ctx, `SELECT request_ref, redacted_by FROM raw_redactions WHERE id = $1`, tombstone).Scan(&ref, &by); err != nil || ref != runbookTicket || by != dbaLogin {
		t.Errorf("tombstone %s = (%q, %q) (error %s), want (%s, %s)", tombstone, ref, by, dbtest.Describe(err), runbookTicket, dbaLogin)
	}

	// The search again no longer returns the redacted id, and still returns the other one.
	if again := runBlock(ctx, t, conn, blocks, "find", fill); len(again) != 1 || len(again[0].Rows) != 1 || cell(t, again[0], 0, 0) != id(alsoNamed) {
		t.Errorf("the find block after the redaction returned rows %v, want only notice %d", rowCounts(again), alsoNamed)
	}

	// Several in one statement, all or none.
	fill["<ID>"], fill["<ID2>"] = id(alsoNamed), id(other)
	many := runBlock(ctx, t, conn, blocks, "redact-many", fill)
	if len(many) != 1 || len(many[0].Rows) != 2 {
		t.Errorf("the redact-many block returned rows %v, want two tombstones", rowCounts(many))
	}
	if got := scanInt(ctx, t, env.admin, `SELECT count(*) FROM raw_redactions`); got != 3 {
		t.Errorf("tombstones = %d after the runbook, want 3", got)
	}

	// After a restore: find the row by its identity, then check that the table and the
	// rows agree.
	identity := map[string]string{"<DOFFIN_ID>": original.DoffinID, "<ORIGINAL_SHA256>": original.BodySHA256}
	if found := runBlock(ctx, t, conn, blocks, "restore-find", identity); len(found) != 1 || len(found[0].Rows) != 1 ||
		cell(t, found[0], 0, 0) != id(named) || cell(t, found[0], 0, 1) != "t" {
		t.Errorf("the restore-find block returned rows %v, want the redacted row %d", rowCounts(found), named)
	}
	for i, r := range runBlock(ctx, t, conn, blocks, "restore-check", nil) {
		if len(r.Rows) != 0 {
			t.Errorf("restore-check query %d returned %d row(s), want none: the tombstones and the rows disagree", i+1, len(r.Rows))
		}
	}
}

func rowCounts(results []*pgconn.Result) []int {
	counts := make([]int, len(results))
	for i, r := range results {
		counts[i] = len(r.Rows)
	}
	return counts
}
