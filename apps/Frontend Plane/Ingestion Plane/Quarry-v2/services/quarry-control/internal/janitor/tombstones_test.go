package janitor

import (
	"context"
	"fmt"
	"math"
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
	// 106752 days is where days*24h overflows an int64 Duration; 999999 and 1000000 are far past it.
	for _, bad := range []string{"-1", "abc", "3.5", "1e3", "36501", "106752", "999999", "1000000", "99999999999999999999"} {
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

// A huge retention must never put the cutoff in the future (a Duration overflow did,
// from 106752 days, and one sweep then deleted every fresh tombstone). The last two
// values matter for the clamp: time.AddDate itself wraps into the future there (1<<55
// and math.MaxInt days), so calendar arithmetic alone is not enough.
func TestSweepWithAHugeRetentionDeletesNothingFresh(t *testing.T) {
	for _, days := range []int{36500, 36501, 100000, 106752, 999999, 1000000, 1 << 40, 1 << 55, math.MaxInt} {
		db := store.NewMemory()
		seedTombstones(t, db, 3, 2, 1)
		if deleted := SweepTombstones(context.Background(), db, opts(days), zerolog.Nop()); deleted != 0 {
			t.Fatalf("retention %d days deleted %d fresh tombstones", days, deleted)
		}
		if got := remaining(t, db); got != 3 {
			t.Fatalf("retention %d days left %d of 3", days, got)
		}
	}
}

func TestTombstoneCutoffNeverLandsInTheFuture(t *testing.T) {
	for _, days := range []int{-5, 0, 1, 30, 36500, 36501, 106752, 1 << 40, 1 << 55, math.MaxInt} {
		if c := tombstoneCutoff(sweepNow, days); c.After(sweepNow) {
			t.Fatalf("%d days: cutoff %v is after now", days, c)
		}
	}
	if c := tombstoneCutoff(sweepNow, -1); !c.Equal(sweepNow.AddDate(0, 0, -DefaultTombstoneRetentionDays)) {
		t.Fatalf("a negative count must be the default, got %v", c)
	}
}

// The sweeper ages out erasure markers, but never sooner than MinErasureMarkerDays,
// however short the tombstone retention is.
func TestSweepKeepsErasureMarkersForAtLeastTheMinimum(t *testing.T) {
	db := store.NewMemory()
	db.ZdrTombstones().SetErasureMarkers(true)
	if _, err := db.PurgeOrg("org_a"); err != nil {
		t.Fatal(err)
	}
	put := func(run string) bool {
		inserted, err := db.ZdrTombstones().Put(store.ZdrTombstone{
			RunID: run, OrgID: "org_a", Kind: "scrape", RetentionClass: store.ZdrTombstoneRetentionClass,
			Outcome: "ok", EndedAt: sweepNow,
		})
		if err != nil {
			t.Fatal(err)
		}
		return inserted
	}
	// The marker was written "now" by the real clock. A sweep whose clock is 6 days
	// ahead, with a 0-day retention, must keep it (7-day minimum).
	o := opts(0)
	real := time.Now()
	o.Now = func() time.Time { return real.AddDate(0, 0, 6) }
	SweepTombstones(context.Background(), db, o, zerolog.Nop())
	if put("run_1") {
		t.Fatal("the marker was swept before MinErasureMarkerDays")
	}
	// 8 days ahead: the marker is gone and the org stores again.
	o.Now = func() time.Time { return real.AddDate(0, 0, 8) }
	SweepTombstones(context.Background(), db, o, zerolog.Nop())
	if !put("run_2") {
		t.Fatal("the marker survived past MinErasureMarkerDays")
	}
}

// A dry run deletes no marker either.
func TestSweepDryRunKeepsErasureMarkers(t *testing.T) {
	db := store.NewMemory()
	db.ZdrTombstones().SetErasureMarkers(true)
	if _, err := db.PurgeOrg("org_a"); err != nil {
		t.Fatal(err)
	}
	o := opts(0)
	o.DryRun = true
	real := time.Now()
	o.Now = func() time.Time { return real.AddDate(0, 0, 30) }
	SweepTombstones(context.Background(), db, o, zerolog.Nop())
	inserted, err := db.ZdrTombstones().Put(store.ZdrTombstone{
		RunID: "run_1", OrgID: "org_a", Kind: "scrape", RetentionClass: store.ZdrTombstoneRetentionClass,
		Outcome: "ok", EndedAt: sweepNow,
	})
	if err != nil || inserted {
		t.Fatalf("a dry run swept the marker: inserted=%v err=%v", inserted, err)
	}
}

// The receiver (and with it the erasure marker) is OFF unless QUARRY_ZDR_TOMBSTONE_RECEIVER
// is a truthy value: a mistyped or empty value must not turn it on.
func TestTombstoneReceiverIsOffByDefault(t *testing.T) {
	env := func(m map[string]string) func(string) string { return func(k string) string { return m[k] } }
	for _, v := range []string{"", "0", "false", "off", "no", "tru", "enabled", " "} {
		if TombstoneReceiverEnabled(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_RECEIVER": v})) {
			t.Fatalf("%q enabled the receiver", v)
		}
	}
	if TombstoneReceiverEnabled(env(nil)) {
		t.Fatal("an unset variable enabled the receiver")
	}
	for _, v := range []string{"true", "1", "yes", "on", "TRUE", " On "} {
		if !TombstoneReceiverEnabled(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_RECEIVER": v})) {
			t.Fatalf("%q did not enable the receiver", v)
		}
	}
	// The other tombstone variables never enable it.
	if TombstoneReceiverEnabled(env(map[string]string{"QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS": "1", "QUARRY_ZDR_TOMBSTONE_DRY_RUN": "true"})) {
		t.Fatal("an unrelated tombstone variable enabled the receiver")
	}
}
