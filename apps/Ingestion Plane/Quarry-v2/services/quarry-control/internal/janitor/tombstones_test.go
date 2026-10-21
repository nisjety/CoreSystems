package janitor

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

var sweepNow = time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC)

func seedTombstones(t *testing.T, db store.DB, agesInDays ...float64) {
	t.Helper()
	for i, age := range agesInDays {
		ended := sweepNow.Add(-time.Duration(age * 24 * float64(time.Hour)))
		_, err := db.ZdrTombstones().Put(store.ZdrTombstone{
			RunID: fmt.Sprintf("run_%d", i), OrgID: "org_a", Kind: "scrape",
			RetentionClass: store.ZdrTombstoneRetentionClass, Outcome: "ok", EndedAt: ended,
		})
		if err != nil {
			t.Fatal(err)
		}
	}
}

func remaining(t *testing.T, db store.DB) int {
	t.Helper()
	rows, err := db.ZdrTombstones().ListByOrg("org_a", 1000)
	if err != nil {
		t.Fatal(err)
	}
	return len(rows)
}

func opts(days int) TombstoneOptions {
	return TombstoneOptions{RetentionDays: days, Batch: 2, Now: func() time.Time { return sweepNow }}
}

// The plan's TestSweepDeletesOnlyPastRetention. Batch 2 forces more than one batch.
func TestSweepDeletesOnlyPastRetention(t *testing.T) {
	db := store.NewMemory()
	seedTombstones(t, db, 90, 45, 31, 29.5, 2, 0.1)
	deleted := SweepTombstones(context.Background(), db, opts(30), zerolog.Nop())
	if deleted != 3 {
		t.Fatalf("deleted %d, want the 3 older than 30 days", deleted)
	}
	if got := remaining(t, db); got != 3 {
		t.Fatalf("%d remain, want the 3 inside the window", got)
	}
	// A second sweep finds nothing more.
	if again := SweepTombstones(context.Background(), db, opts(30), zerolog.Nop()); again != 0 {
		t.Fatalf("a repeat sweep deleted %d", again)
	}
}

// 0 days is how the evidence step ages rows out at once.
func TestSweepZeroDaysDeletesAll(t *testing.T) {
	db := store.NewMemory()
	seedTombstones(t, db, 90, 3, 0.5, 0.01)
	if deleted := SweepTombstones(context.Background(), db, opts(0), zerolog.Nop()); deleted != 4 {
		t.Fatalf("deleted %d, want 4", deleted)
	}
	if got := remaining(t, db); got != 0 {
		t.Fatalf("%d remain after a 0-day sweep", got)
	}
}

func TestSweepDryRunDeletesNothing(t *testing.T) {
	db := store.NewMemory()
	seedTombstones(t, db, 90, 45, 1)
	o := opts(30)
	o.DryRun = true
	if deleted := SweepTombstones(context.Background(), db, o, zerolog.Nop()); deleted != 0 {
		t.Fatalf("a dry run reported %d deleted", deleted)
	}
	if got := remaining(t, db); got != 3 {
		t.Fatalf("a dry run deleted rows: %d remain", got)
	}
}

func TestTombstoneOptionsFromEnv(t *testing.T) {
	env := func(m map[string]string) func(string) string { return func(k string) string { return m[k] } }

	o, invalid := TombstoneOptionsFromEnv(env(nil))
	if invalid || o.RetentionDays != 30 || o.DryRun || o.Interval != time.Hour || o.Batch != 1000 {
		t.Fatalf("defaults wrong: %+v invalid=%v", o, invalid)
	}
	o, invalid = TombstoneOptionsFromEnv(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS": "7", "QUARRY_ZDR_TOMBSTONE_DRY_RUN": "true"}))
	if invalid || o.RetentionDays != 7 || !o.DryRun {
		t.Fatalf("explicit values wrong: %+v", o)
	}
	o, invalid = TombstoneOptionsFromEnv(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS": "0"}))
	if invalid || o.RetentionDays != 0 {
		t.Fatalf("0 must mean 0 days, got %+v invalid=%v", o, invalid)
	}
	// An invalid value never disables the sweeper: it falls back to 30 and says so.
	for _, bad := range []string{"-1", "abc", "3.5", "1e3"} {
		o, invalid = TombstoneOptionsFromEnv(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS": bad}))
		if !invalid || o.RetentionDays != 30 {
			t.Fatalf("%q: want fallback 30 and invalid=true, got %+v invalid=%v", bad, o, invalid)
		}
	}
	// The dry run is read leniently (a mistyped value errs toward not deleting); unset is a real sweep.
	for _, v := range []string{"true", "TRUE", "1", "yes", "on"} {
		o, _ = TombstoneOptionsFromEnv(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_DRY_RUN": v}))
		if !o.DryRun {
			t.Fatalf("%q must turn the dry run on", v)
		}
	}
	o, _ = TombstoneOptionsFromEnv(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_DRY_RUN": "false"}))
	if o.DryRun {
		t.Fatal("false must leave the sweeper real")
	}
}
