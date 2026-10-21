package pg

import (
	"context"
	"encoding/json"
	"os"
	"reflect"
	"regexp"
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

var columnLine = regexp.MustCompile(`(?m)^ {4}([a-z_]+)\s+(?:TEXT|TIMESTAMPTZ|INT|NUMERIC)`)

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
	for _, m := range columnLine.FindAllStringSubmatch(body, -1) {
		got = append(got, m[1])
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
