package pg

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func readMigration015(t *testing.T) string {
	t.Helper()
	b, err := migrationFS.ReadFile("migrations/015_zdr_run_tombstones.sql")
	if err != nil {
		t.Fatalf("read 015: %v", err)
	}
	return strings.ReplaceAll(string(b), "\r\n", "\n")
}

// tableColumns parses the CREATE TABLE IF NOT EXISTS <table> (...) statement in sql
// and returns every column name with its upper-cased base type, whatever the type
// is (TEXT, JSONB, VARCHAR(200), BIGINT, ...). Table constraints are skipped. Comment
// lines are dropped first. It fails the test if the statement is missing or its
// parentheses do not balance, so a column can never hide from the guard by its type.
func tableColumns(t *testing.T, sql, table string) map[string]string {
	t.Helper()
	var code strings.Builder
	for _, line := range strings.Split(sql, "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		code.WriteString(line + "\n")
	}
	body := code.String()
	head := "CREATE TABLE IF NOT EXISTS " + table + " ("
	start := strings.Index(body, head)
	if start < 0 {
		t.Fatalf("no %q in the migration", head)
	}
	rest := body[start+len(head):]
	depth, end := 1, -1
	var items []string
	cur := strings.Builder{}
	for i := 0; i < len(rest) && end < 0; i++ {
		c := rest[i]
		switch {
		case c == '(':
			depth++
			cur.WriteByte(c)
		case c == ')':
			depth--
			if depth == 0 {
				end = i
				continue
			}
			cur.WriteByte(c)
		case c == ',' && depth == 1:
			items = append(items, cur.String())
			cur.Reset()
		default:
			cur.WriteByte(c)
		}
	}
	if end < 0 {
		t.Fatalf("unbalanced parentheses in the CREATE TABLE for %s", table)
	}
	items = append(items, cur.String())
	skip := map[string]bool{"CONSTRAINT": true, "PRIMARY": true, "UNIQUE": true, "CHECK": true, "FOREIGN": true, "EXCLUDE": true, "LIKE": true}
	cols := map[string]string{}
	for _, it := range items {
		f := strings.Fields(it)
		if len(f) == 0 || skip[strings.ToUpper(f[0])] {
			continue
		}
		if len(f) < 2 {
			t.Fatalf("column definition %q has no type", strings.TrimSpace(it))
		}
		typ := strings.ToUpper(f[1])
		if i := strings.IndexByte(typ, '('); i >= 0 {
			typ = typ[:i]
		}
		cols[f[0]] = typ
	}
	return cols
}

// The migration's columns are exactly the schema's keys plus created_at, its CHECK
// lists carry every closed vocabulary, and it has the immutability trigger on UPDATE
// only. Needs no database, so it holds in every CI lane.
func TestMigration015MatchesTheAllowlistSchema(t *testing.T) {
	sql := readMigration015(t)
	// Drop comment lines so a column named only in a comment does not count.
	var code strings.Builder
	for _, line := range strings.Split(sql, "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		code.WriteString(line + "\n")
	}
	body := code.String()

	raw, err := os.ReadFile("../../resources/testdata/zdr-tombstone.schema.json")
	if err != nil {
		t.Fatal(err)
	}
	var schema struct {
		Properties map[string]json.RawMessage `json:"properties"`
	}
	if err := json.Unmarshal(raw, &schema); err != nil {
		t.Fatal(err)
	}
	want := []string{"created_at"}
	for k := range schema.Properties {
		want = append(want, k)
	}
	sort.Strings(want)

	var got []string
	for name := range tableColumns(t, sql, "zdr_run_tombstones") {
		got = append(got, name)
	}
	sort.Strings(got)
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("migration columns %v differ from schema keys plus created_at %v", got, want)
	}

	for _, list := range [][]string{
		store.ZdrTombstoneKinds, store.ZdrTombstoneOutcomes, store.ZdrTombstoneDrivers, store.ZdrTombstoneErrorClasses,
	} {
		for _, v := range list {
			if !strings.Contains(body, "'"+v+"'") {
				t.Fatalf("the migration's CHECK lists do not carry %q", v)
			}
		}
	}
	upper := strings.ToUpper(body)
	for _, must := range []string{
		"CREATE TABLE IF NOT EXISTS ZDR_RUN_TOMBSTONES",
		"RETENTION_CLASS = 'ZDR'",
		"BEFORE UPDATE ON ZDR_RUN_TOMBSTONES",
		"DROP TRIGGER IF EXISTS",
		"CREATE OR REPLACE FUNCTION",
		"(ORG_ID, ENDED_AT DESC)",
		"(ENDED_AT)",
	} {
		if !strings.Contains(upper, must) {
			t.Fatalf("the migration lacks %q", must)
		}
	}
	// The trigger refuses UPDATE only: the sweeper and org erasure DELETE.
	if strings.Contains(upper, "BEFORE DELETE") || strings.Contains(upper, "UPDATE OR DELETE") || strings.Contains(upper, "DELETE OR UPDATE") {
		t.Fatal("the trigger must refuse UPDATE only; DELETE stays allowed for the sweeper and erasure")
	}
	if strings.Contains(upper, "REFERENCES") {
		t.Fatal("a tombstone must not reference another table")
	}
}

func tombstonePool(t *testing.T) (store.DB, *postgresDB) {
	t.Helper()
	db := openOrSkip(t)
	p, ok := db.(*postgresDB)
	if !ok {
		t.Skip("not a postgres DB")
	}
	// What cmd/control does with QUARRY_ZDR_TOMBSTONE_RECEIVER; the default is off.
	db.ZdrTombstones().SetErasureMarkers(true)
	return db, p
}

func pgTombstone(run, org string, ended time.Time) store.ZdrTombstone {
	return store.ZdrTombstone{
		RunID: run, OrgID: org, Kind: "agent_run", RetentionClass: store.ZdrTombstoneRetentionClass,
		Outcome: "ok", EndedAt: ended,
	}
}

func uniq(prefix string) string {
	return prefix + "_" + strings.ReplaceAll(quarrycontracts.NewID(quarrycontracts.KindRun).String(), ".", "_")
}

// The plan's TestZdrTombstoneRowCannotBeUpdated (Postgres, under QUARRY_REQUIRE_DB in CI).
func TestZdrTombstoneRowCannotBeUpdated(t *testing.T) {
	db, p := tombstonePool(t)
	run, org := uniq("run_imm"), uniq("org_imm")
	if _, err := db.ZdrTombstones().Put(pgTombstone(run, org, time.Now().UTC())); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	for _, stmt := range []string{
		`UPDATE zdr_run_tombstones SET outcome = 'error' WHERE run_id = $1`,
		`UPDATE zdr_run_tombstones SET org_id = 'other' WHERE run_id = $1`,
		`UPDATE zdr_run_tombstones SET step_count = 9 WHERE run_id = $1`,
	} {
		if _, err := p.pool.Exec(ctx, stmt, run); err == nil {
			t.Fatalf("an UPDATE reached a tombstone: %s", stmt)
		}
	}
	// A conflicting insert is a no-op and never trips the trigger.
	again := pgTombstone(run, org, time.Now().UTC())
	again.Outcome = "error"
	if inserted, err := db.ZdrTombstones().Put(again); err != nil || inserted {
		t.Fatalf("duplicate Put: inserted=%v err=%v", inserted, err)
	}
	rows, _ := db.ZdrTombstones().ListByOrg(org, 10)
	if len(rows) != 1 || rows[0].Outcome != "ok" || rows[0].Kind != "agent_run" {
		t.Fatalf("first write must win: %+v", rows)
	}
	// DELETE stays allowed (the sweeper and org erasure).
	if _, err := p.pool.Exec(ctx, `DELETE FROM zdr_run_tombstones WHERE run_id = $1`, run); err != nil {
		t.Fatalf("DELETE must be allowed: %v", err)
	}
}

func TestZdrTombstoneMigrationAppliesTwice(t *testing.T) {
	db, p := tombstonePool(t)
	sql := readMigration015(t)
	run, org := uniq("run_mig"), uniq("org_mig")
	if _, err := db.ZdrTombstones().Put(pgTombstone(run, org, time.Now().UTC())); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	for i := 0; i < 2; i++ {
		if _, err := p.pool.Exec(ctx, sql); err != nil {
			t.Fatalf("re-apply %d: %v", i, err)
		}
	}
	if rows, _ := db.ZdrTombstones().ListByOrg(org, 10); len(rows) != 1 {
		t.Fatalf("the row did not survive a re-apply: %+v", rows)
	}
	// Exactly one immutability trigger remains, and it still refuses an UPDATE.
	var n int
	if err := p.pool.QueryRow(ctx, `SELECT count(*) FROM pg_trigger WHERE tgrelid = 'zdr_run_tombstones'::regclass AND NOT tgisinternal`).Scan(&n); err != nil || n != 1 {
		t.Fatalf("want exactly 1 trigger after two re-applies, got %d (%v)", n, err)
	}
	if _, err := p.pool.Exec(ctx, `UPDATE zdr_run_tombstones SET outcome = 'error' WHERE run_id = $1`, run); err == nil {
		t.Fatal("the trigger stopped refusing UPDATE after a re-apply")
	}
	_, _ = p.pool.Exec(ctx, `DELETE FROM zdr_run_tombstones WHERE run_id = $1`, run)
}

// The table refuses a value outside the vocabulary whatever wrote it.
func TestZdrTombstoneTableRefusesValuesOutsideTheVocabulary(t *testing.T) {
	_, p := tombstonePool(t)
	ctx := context.Background()
	org := uniq("org_chk")
	insert := func(override map[string]string) error {
		cols := map[string]string{
			"run_id": uniq("run_chk"), "org_id": org, "kind": "scrape", "retention_class": "zdr",
			"outcome": "ok", "driver": "static", "error_class": "TIMEOUT",
		}
		for k, v := range override {
			cols[k] = v
		}
		_, err := p.pool.Exec(ctx, `INSERT INTO zdr_run_tombstones (run_id, org_id, kind, retention_class, outcome, driver, ended_at, error_class)
			VALUES ($1,$2,$3,$4,$5,$6, now(), $7)`,
			cols["run_id"], cols["org_id"], cols["kind"], cols["retention_class"], cols["outcome"], cols["driver"], cols["error_class"])
		return err
	}
	if err := insert(nil); err != nil {
		t.Fatalf("the control row must insert: %v", err)
	}
	defer func() { _, _ = p.pool.Exec(ctx, `DELETE FROM zdr_run_tombstones WHERE org_id = $1`, org) }()
	for _, bad := range []map[string]string{
		{"run_id": "https://example.com/x"}, {"retention_class": "standard"}, {"driver": "example.com"},
		{"error_class": "connection refused by 10.0.0.1"}, {"kind": "crawl"}, {"outcome": "fine"},
		{"run_id": strings.Repeat("a", 65)}, {"org_id": "org/with/slash"},
	} {
		if err := insert(bad); err == nil {
			t.Fatalf("the table accepted %v", bad)
		}
	}
}

func TestPgZdrTombstoneStoreRoundTrip(t *testing.T) {
	db, _ := tombstonePool(t)
	org := uniq("org_rt")
	now := time.Now().UTC().Truncate(time.Millisecond)
	started := now.Add(-2 * time.Second)
	step, cost := 7, 1.5
	driver, class := "browser", "TIMEOUT"
	full := pgTombstone(uniq("run_full"), org, now)
	full.Outcome, full.Driver, full.StartedAt, full.StepCount, full.CostUnits, full.ErrorClass = "timeout", &driver, &started, &step, &cost, &class
	old := pgTombstone(uniq("run_old"), org, now.Add(-48*time.Hour))
	older := pgTombstone(uniq("run_older"), org, now.Add(-72*time.Hour))
	for _, z := range []store.ZdrTombstone{full, old, older} {
		if inserted, err := db.ZdrTombstones().Put(z); err != nil || !inserted {
			t.Fatalf("put %s: inserted=%v err=%v", z.RunID, inserted, err)
		}
	}
	rows, err := db.ZdrTombstones().ListByOrg(org, 10)
	if err != nil || len(rows) != 3 || rows[0].RunID != full.RunID {
		t.Fatalf("list newest first: %+v err=%v", rows, err)
	}
	got := rows[0]
	if got.Driver == nil || *got.Driver != "browser" || got.StepCount == nil || *got.StepCount != 7 ||
		got.CostUnits == nil || *got.CostUnits != 1.5 || got.ErrorClass == nil || *got.ErrorClass != "TIMEOUT" ||
		got.StartedAt == nil || !got.StartedAt.Equal(started) || !got.EndedAt.Equal(now) || got.Outcome != "timeout" {
		t.Fatalf("round trip lost a field: %+v", got)
	}
	if rows[1].Driver != nil || rows[1].StepCount != nil || rows[1].CostUnits != nil || rows[1].ErrorClass != nil || rows[1].StartedAt != nil {
		t.Fatalf("absent optional fields must come back absent: %+v", rows[1])
	}
	cutoff := now.Add(-24 * time.Hour)
	if n, err := db.ZdrTombstones().CountBefore(cutoff); err != nil || n < 2 {
		t.Fatalf("CountBefore = %d err=%v, want at least the 2 old rows", n, err)
	}
	if n, err := db.ZdrTombstones().DeleteBefore(now.Add(-60*time.Hour), 100); err != nil || n < 1 {
		t.Fatalf("DeleteBefore: n=%d err=%v", n, err)
	}
	rows, _ = db.ZdrTombstones().ListByOrg(org, 10)
	if len(rows) != 2 {
		t.Fatalf("the 72h-old row must be gone and the 48h-old one kept, got %d rows", len(rows))
	}
	if _, err := db.PurgeOrg(org); err != nil {
		t.Fatal(err)
	}
}

// The plan's TestOrgErasureRemovesTombstones, against the real DELETE statement.
func TestPostgresOrgErasureRemovesTombstones(t *testing.T) {
	db, _ := tombstonePool(t)
	a, b := uniq("org_era_a"), uniq("org_era_b")
	for _, z := range []store.ZdrTombstone{
		pgTombstone(uniq("run_a1"), a, time.Now().UTC()), pgTombstone(uniq("run_a2"), a, time.Now().UTC()),
		pgTombstone(uniq("run_b1"), b, time.Now().UTC()),
	} {
		if _, err := db.ZdrTombstones().Put(z); err != nil {
			t.Fatal(err)
		}
	}
	res, err := db.PurgeOrg(a)
	if err != nil || res.ZdrTombstonesDeleted != 2 {
		t.Fatalf("PurgeOrg: %+v err=%v", res, err)
	}
	if rows, _ := db.ZdrTombstones().ListByOrg(a, 10); len(rows) != 0 {
		t.Fatalf("org a tombstones survived erasure: %+v", rows)
	}
	if rows, _ := db.ZdrTombstones().ListByOrg(b, 10); len(rows) != 1 {
		t.Fatalf("org b must keep its tombstone: %+v", rows)
	}
	if _, err := db.PurgeOrg(b); err != nil {
		t.Fatal(err)
	}
}

// A run that ends after its org was erased must not re-create a row (review finding),
// against the real statements, including the marker's sweep.
func TestPostgresTombstoneAfterOrgErasureIsNotStored(t *testing.T) {
	db, p := tombstonePool(t)
	org, other := uniq("org_late"), uniq("org_late_other")
	now := time.Now().UTC()
	if inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_pre"), org, now)); err != nil || !inserted {
		t.Fatalf("put before erasure: inserted=%v err=%v", inserted, err)
	}
	if res, err := db.PurgeOrg(org); err != nil || res.ZdrTombstonesDeleted != 1 {
		t.Fatalf("PurgeOrg: %+v err=%v", res, err)
	}
	inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_post"), org, now))
	if err != nil || inserted {
		t.Fatalf("a tombstone for an erased org: inserted=%v err=%v, want the silent no-op", inserted, err)
	}
	if rows, _ := db.ZdrTombstones().ListByOrg(org, 10); len(rows) != 0 {
		t.Fatalf("the erased org has rows again: %+v", rows)
	}
	if inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_other"), other, now)); err != nil || !inserted {
		t.Fatalf("another org must still store: inserted=%v err=%v", inserted, err)
	}
	// A repeat erasure refreshes the marker (no unique-violation) and stays a zero result.
	if res, err := db.PurgeOrg(org); err != nil || res.Total() != 0 {
		t.Fatalf("repeat PurgeOrg: %+v err=%v", res, err)
	}
	// A fresh marker survives a sweep with a cutoff in the past.
	if _, err := db.ZdrTombstones().DeleteErasureMarkersBefore(now.Add(-time.Hour)); err != nil {
		t.Fatalf("sweep with a past cutoff: %v", err)
	}
	var markers int
	if err := p.pool.QueryRow(context.Background(), `SELECT count(*) FROM zdr_tombstone_erasures WHERE org_id = $1`, org).Scan(&markers); err != nil || markers != 1 {
		t.Fatalf("the fresh marker must survive a past cutoff: markers=%d err=%v", markers, err)
	}
	// An old one is swept, and the org stores again.
	if _, err := db.ZdrTombstones().DeleteErasureMarkersBefore(time.Now().UTC().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	if inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_again"), org, now)); err != nil || !inserted {
		t.Fatalf("after the marker is swept the org stores again: inserted=%v err=%v", inserted, err)
	}
	_, _ = db.PurgeOrg(org)
	_, _ = db.PurgeOrg(other)
	_, _ = p.pool.Exec(context.Background(), `DELETE FROM zdr_tombstone_erasures WHERE org_id IN ($1, $2)`, org, other)
}

// Put and PurgeOrg for the same org are serialised: whatever order they interleave
// in, no row for the org survives once the last erasure has committed and no Put is
// still running.
func TestPostgresPutRacingOrgErasureLeavesNoRow(t *testing.T) {
	db, p := tombstonePool(t)
	org := uniq("org_race")
	now := time.Now().UTC()
	done := make(chan struct{})
	go func() {
		defer close(done)
		for i := 0; i < 40; i++ {
			_, _ = db.ZdrTombstones().Put(pgTombstone(uniq("run_race"), org, now))
		}
	}()
	for i := 0; i < 10; i++ {
		if _, err := db.PurgeOrg(org); err != nil {
			t.Fatal(err)
		}
	}
	<-done
	// The last Put may have run before the first erasure; one more erasure now removes
	// anything that landed, and after it nothing can land.
	if _, err := db.PurgeOrg(org); err != nil {
		t.Fatal(err)
	}
	if inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_race_late"), org, now)); err != nil || inserted {
		t.Fatalf("a Put after the erasure stored a row: inserted=%v err=%v", inserted, err)
	}
	if rows, _ := db.ZdrTombstones().ListByOrg(org, 100); len(rows) != 0 {
		t.Fatalf("%d rows for an erased org", len(rows))
	}
	_, _ = p.pool.Exec(context.Background(), `DELETE FROM zdr_tombstone_erasures WHERE org_id = $1`, org)
}

func readMigration024(t *testing.T) string {
	t.Helper()
	b, err := migrationFS.ReadFile("migrations/024_zdr_tombstone_erasures.sql")
	if err != nil {
		t.Fatalf("read 024: %v", err)
	}
	return strings.ReplaceAll(string(b), "\r\n", "\n")
}

// The erasure marker table holds an org id and a timestamp and nothing else: a column
// of ANY type (TEXT, JSONB, VARCHAR, BIGINT, ...) added by accident fails this test,
// because every column definition in the CREATE TABLE is compared with a golden list
// of names and types. Needs no database, so it holds in every CI lane.
func TestMigration024HoldsOnlyAnOrgIDAndATimestamp(t *testing.T) {
	var code strings.Builder
	for _, line := range strings.Split(readMigration024(t), "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		code.WriteString(line + "\n")
	}
	got := tableColumns(t, readMigration024(t), "zdr_tombstone_erasures")
	if !reflect.DeepEqual(got, map[string]string{"org_id": "TEXT", "erased_at": "TIMESTAMPTZ"}) {
		t.Fatalf("zdr_tombstone_erasures columns = %v, want exactly org_id TEXT and erased_at TIMESTAMPTZ", got)
	}
	upper := strings.ToUpper(code.String())
	for _, banned := range []string{"REFERENCES", "TRIGGER", "DROP "} {
		if strings.Contains(upper, banned) {
			t.Fatalf("migration 024 must stay a plain idempotent CREATE ... IF NOT EXISTS, found %q", banned)
		}
	}
	for _, create := range []string{"CREATE TABLE IF NOT EXISTS", "CREATE INDEX IF NOT EXISTS"} {
		if !strings.Contains(upper, create) {
			t.Fatalf("migration 024 lost %q (it must be idempotent)", create)
		}
	}
}

func TestMigration024AppliesTwice(t *testing.T) {
	db, p := tombstonePool(t)
	sql := readMigration024(t)
	org := uniq("org_m24")
	ctx := context.Background()
	if _, err := db.PurgeOrg(org); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		if _, err := p.pool.Exec(ctx, sql); err != nil {
			t.Fatalf("re-apply %d: %v", i, err)
		}
	}
	var n int
	if err := p.pool.QueryRow(ctx, `SELECT count(*) FROM zdr_tombstone_erasures WHERE org_id = $1`, org).Scan(&n); err != nil || n != 1 {
		t.Fatalf("the marker did not survive a re-apply: n=%d err=%v", n, err)
	}
	// The shape CHECK refuses an id a tombstone could never carry.
	if _, err := p.pool.Exec(ctx, `INSERT INTO zdr_tombstone_erasures (org_id) VALUES ('https://example.org/x')`); err == nil {
		t.Fatal("the marker table accepted an org id outside the tombstone shape")
	}
	_, _ = p.pool.Exec(ctx, `DELETE FROM zdr_tombstone_erasures WHERE org_id = $1`, org)
}

// An org id that no tombstone can carry gets no marker (the erasure still succeeds).
func TestPostgresErasureMarkerOnlyForATombstoneShapedOrgID(t *testing.T) {
	db, p := tombstonePool(t)
	ctx := context.Background()
	odd := "org with spaces " + uniq("x")
	if _, err := db.PurgeOrg(odd); err != nil {
		t.Fatalf("PurgeOrg of a non-tombstone-shaped id must still succeed: %v", err)
	}
	var n int
	if err := p.pool.QueryRow(ctx, `SELECT count(*) FROM zdr_tombstone_erasures WHERE org_id = $1`, odd).Scan(&n); err != nil || n != 0 {
		t.Fatalf("a marker was written for a non-tombstone-shaped id: n=%d err=%v", n, err)
	}
}

// Put takes the same advisory lock as PurgeOrg. While an erasure transaction holds it
// (marker written, not yet committed) a Put must wait, and once the erasure commits
// the Put reports the refused no-op. Without the lock the Put would read "no marker"
// from before the commit and store its row after the erasure's DELETE.
func TestPostgresPutBlocksOnTheOrgLockHeldByAnErasure(t *testing.T) {
	db, p := tombstonePool(t)
	ctx := context.Background()
	org := uniq("org_lock")
	tx, err := p.pool.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, orgLockSQL, org); err != nil {
		t.Fatal(err)
	}
	if _, err := tx.Exec(ctx, `INSERT INTO zdr_tombstone_erasures (org_id) VALUES ($1)`, org); err != nil {
		t.Fatal(err)
	}
	type result struct {
		inserted bool
		err      error
	}
	res := make(chan result, 1)
	go func() {
		inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_lock"), org, time.Now().UTC()))
		res <- result{inserted, err}
	}()
	select {
	case r := <-res:
		t.Fatalf("Put returned while the erasure held the org lock: %+v", r)
	case <-time.After(700 * time.Millisecond):
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatal(err)
	}
	select {
	case r := <-res:
		if r.err != nil || r.inserted {
			t.Fatalf("after the erasure committed the Put must be the refused no-op: %+v", r)
		}
	case <-time.After(15 * time.Second):
		t.Fatal("Put never returned after the erasure committed")
	}
	if rows, _ := db.ZdrTombstones().ListByOrg(org, 10); len(rows) != 0 {
		t.Fatalf("a row exists for the erased org: %+v", rows)
	}
	_, _ = p.pool.Exec(ctx, `DELETE FROM zdr_tombstone_erasures WHERE org_id = $1`, org)
}

// Erasure markers are swept by age only.
func TestPostgresErasureMarkerSweepIsByAge(t *testing.T) {
	db, p := tombstonePool(t)
	ctx := context.Background()
	oldOrg, newOrg := uniq("org_old"), uniq("org_new")
	if _, err := p.pool.Exec(ctx, `INSERT INTO zdr_tombstone_erasures (org_id, erased_at) VALUES ($1, now() - interval '40 days'), ($2, now())`, oldOrg, newOrg); err != nil {
		t.Fatal(err)
	}
	n, err := db.ZdrTombstones().DeleteErasureMarkersBefore(time.Now().UTC().Add(-30 * 24 * time.Hour))
	if err != nil || n < 1 {
		t.Fatalf("sweep: n=%d err=%v, want the 40-day-old marker removed", n, err)
	}
	var old, fresh int
	_ = p.pool.QueryRow(ctx, `SELECT count(*) FROM zdr_tombstone_erasures WHERE org_id = $1`, oldOrg).Scan(&old)
	_ = p.pool.QueryRow(ctx, `SELECT count(*) FROM zdr_tombstone_erasures WHERE org_id = $1`, newOrg).Scan(&fresh)
	if old != 0 || fresh != 1 {
		t.Fatalf("old=%d fresh=%d, want 0 and 1", old, fresh)
	}
	_, _ = p.pool.Exec(ctx, `DELETE FROM zdr_tombstone_erasures WHERE org_id = $1`, newOrg)
}

// PurgeOrg takes the same lock: it must wait for a Put (or anything) that holds it,
// so the erasure's DELETE and marker never interleave with a tombstone insert.
func TestPostgresPurgeOrgBlocksOnTheOrgLock(t *testing.T) {
	db, p := tombstonePool(t)
	ctx := context.Background()
	org := uniq("org_plock")
	tx, err := p.pool.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, orgLockSQL, org); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() {
		_, err := db.PurgeOrg(org)
		done <- err
	}()
	select {
	case err := <-done:
		t.Fatalf("PurgeOrg returned while the org lock was held: err=%v", err)
	case <-time.After(700 * time.Millisecond):
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("PurgeOrg after the lock was released: %v", err)
		}
	case <-time.After(15 * time.Second):
		t.Fatal("PurgeOrg never returned after the lock was released")
	}
	_, _ = p.pool.Exec(ctx, `DELETE FROM zdr_tombstone_erasures WHERE org_id = $1`, org)
}

// With the marker write off (the default) PurgeOrg leaves no row in
// zdr_tombstone_erasures and a Put for the org is stored: a fresh deploy retains
// nothing of an erased org's id.
func TestPostgresPurgeOrgWritesNoMarkerByDefault(t *testing.T) {
	db := openOrSkip(t)
	p, ok := db.(*postgresDB)
	if !ok {
		t.Skip("not a postgres DB")
	}
	if p.zdrTombs.markers.Load() {
		t.Fatal("the marker write is on by default")
	}
	ctx := context.Background()
	org := uniq("org_nomark")
	if _, err := db.PurgeOrg(org); err != nil {
		t.Fatal(err)
	}
	var n int
	if err := p.pool.QueryRow(ctx, `SELECT count(*) FROM zdr_tombstone_erasures WHERE org_id = $1`, org).Scan(&n); err != nil || n != 0 {
		t.Fatalf("a marker was written with the switch off: n=%d err=%v", n, err)
	}
	if inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_nomark"), org, time.Now().UTC())); err != nil || !inserted {
		t.Fatalf("a Put after an unmarked erasure: inserted=%v err=%v", inserted, err)
	}
	_, _ = db.PurgeOrg(org)
}

// While an erasure holds an org's lock, a burst of Puts for that org (many more than
// the pool has connections) must neither wait for the erasure to commit nor take the
// pool's connections: the rest of quarry-control keeps running, every Put gives up
// with ErrZdrTombstoneBusy, and the pool never has more than 4 connections in use by
// them (a quarter of its 16).
func TestPostgresPutBurstBehindAnErasureCannotStarveThePool(t *testing.T) {
	db, p := tombstonePool(t)
	ctx := context.Background()
	org := uniq("org_burst")
	p.zdrTombs.lockTimeout = 400 * time.Millisecond
	p.zdrTombs.putTimeout = 2 * time.Second

	tx, err := p.pool.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, orgLockSQL, org); err != nil {
		t.Fatal(err)
	}

	const burst = 48 // three times the pool's 16 connections
	errs := make(chan error, burst)
	start := time.Now()
	for i := 0; i < burst; i++ {
		go func(i int) {
			_, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_burst"), org, time.Now().UTC()))
			errs <- err
		}(i)
	}

	// While the burst waits, the pool still serves everything else, and the Puts hold
	// at most zdrPutSlots connections.
	maxHeld := int32(0)
	deadline := time.Now().Add(1500 * time.Millisecond)
	for time.Now().Before(deadline) {
		if held := p.pool.Stat().AcquiredConns() - 1; held > maxHeld { // minus the erasure tx
			maxHeld = held
		}
		qctx, cancel := context.WithTimeout(ctx, time.Second)
		var one int
		if err := p.pool.QueryRow(qctx, `SELECT 1`).Scan(&one); err != nil || one != 1 {
			cancel()
			t.Fatalf("the pool was starved by the burst: %v", err)
		}
		cancel()
		time.Sleep(50 * time.Millisecond)
	}
	// A literal, not zdrPutSlots: the bound the runbook promises is 4, and the test must
	// fail if the constant is raised toward the pool size.
	if maxHeld > 4 {
		t.Fatalf("Puts held %d connections, want at most 4", maxHeld)
	}

	for i := 0; i < burst; i++ {
		select {
		case err := <-errs:
			if !errors.Is(err, ErrZdrTombstoneBusy) {
				t.Fatalf("a Put behind the erasure returned %v, want ErrZdrTombstoneBusy", err)
			}
		case <-time.After(10 * time.Second):
			t.Fatalf("Put %d never returned although the erasure still held the lock", i)
		}
	}
	if took := time.Since(start); took > 6*time.Second {
		t.Fatalf("the burst took %v, the bound is the put timeout", took)
	}
	if rows, _ := db.ZdrTombstones().ListByOrg(org, 100); len(rows) != 0 {
		t.Fatalf("%d rows were stored behind the lock", len(rows))
	}
	// Released, the next Put works again.
	if err := tx.Commit(ctx); err != nil {
		t.Fatal(err)
	}
	p.zdrTombs.lockTimeout, p.zdrTombs.putTimeout = zdrPutLockTimeout, zdrPutTimeout
	if inserted, err := db.ZdrTombstones().Put(pgTombstone(uniq("run_after"), org, time.Now().UTC())); err != nil || !inserted {
		t.Fatalf("a Put after the lock was released: inserted=%v err=%v", inserted, err)
	}
	_, _ = db.PurgeOrg(org)
}
