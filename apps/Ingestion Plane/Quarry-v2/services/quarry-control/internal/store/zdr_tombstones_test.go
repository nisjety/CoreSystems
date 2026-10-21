package store

import (
	"errors"
	"math"
	"strings"
	"testing"
	"time"
)

func goodTombstone(run, org string, ended time.Time) ZdrTombstone {
	return ZdrTombstone{
		RunID: run, OrgID: org, Kind: "scrape", RetentionClass: ZdrTombstoneRetentionClass,
		Outcome: "ok", EndedAt: ended,
	}
}

func strp(s string) *string { return &s }

func TestZdrTombstoneValidateRefusesEveryValueOutsideTheAllowlist(t *testing.T) {
	t.Parallel()
	now := time.Now().UTC()
	neg := -1
	big := 1e12
	nan := math.NaN()
	later := now.Add(time.Hour)
	cases := map[string]func(*ZdrTombstone){
		"run_id":          func(z *ZdrTombstone) { z.RunID = "https://example.com/x" },
		"run_id empty":    func(z *ZdrTombstone) { z.RunID = "" },
		"run_id too long": func(z *ZdrTombstone) { z.RunID = string(make([]byte, 65)) },
		"run_id space":    func(z *ZdrTombstone) { z.RunID = "run 1" },
		"org_id":          func(z *ZdrTombstone) { z.OrgID = "org/1" },
		"kind":            func(z *ZdrTombstone) { z.Kind = "crawl" },
		"retention_class": func(z *ZdrTombstone) { z.RetentionClass = "standard" },
		"outcome":         func(z *ZdrTombstone) { z.Outcome = "success" },
		"driver":          func(z *ZdrTombstone) { z.Driver = strp("example.com") },
		"ended_at":        func(z *ZdrTombstone) { z.EndedAt = time.Time{} },
		"started_at":      func(z *ZdrTombstone) { z.StartedAt = &later },
		"step_count":      func(z *ZdrTombstone) { z.StepCount = &neg },
		"cost_units neg":  func(z *ZdrTombstone) { v := -0.5; z.CostUnits = &v },
		"cost_units big":  func(z *ZdrTombstone) { z.CostUnits = &big },
		"cost_units nan":  func(z *ZdrTombstone) { z.CostUnits = &nan },
		"error_class":     func(z *ZdrTombstone) { z.ErrorClass = strp("connection refused to 10.0.0.1:443") },
	}
	for name, mutate := range cases {
		z := goodTombstone("run_1", "org_1", now)
		mutate(&z)
		err := z.Validate()
		if err == nil {
			t.Fatalf("%s: a value outside the allowlist was accepted", name)
		}
		if !errors.Is(err, ErrZdrTombstoneInvalid) {
			t.Fatalf("%s: wrong error %v", name, err)
		}
	}
	if err := goodTombstone("run_1", "org_1", now).Validate(); err != nil {
		t.Fatalf("a minimal tombstone must validate: %v", err)
	}
}

func TestZdrTombstoneValidateErrorNeverEchoesTheValue(t *testing.T) {
	t.Parallel()
	z := goodTombstone("https://secret.example/page?q=1", "org_1", time.Now())
	err := z.Validate()
	if err == nil {
		t.Fatal("expected an error")
	}
	for _, leak := range []string{"secret.example", "q=1", "https"} {
		if strings.Contains(err.Error(), leak) {
			t.Fatalf("the error echoes the value (%q): %v", leak, err)
		}
	}
}


func TestMemoryZdrTombstonePutIsFirstWriteWins(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	now := time.Now().UTC()
	first := goodTombstone("run_dup", "org_a", now)
	second := goodTombstone("run_dup", "org_a", now)
	second.Outcome = "error"
	if inserted, err := db.ZdrTombstones().Put(first); err != nil || !inserted {
		t.Fatalf("first put: inserted=%v err=%v", inserted, err)
	}
	if inserted, err := db.ZdrTombstones().Put(second); err != nil || inserted {
		t.Fatalf("second put must be a no-op: inserted=%v err=%v", inserted, err)
	}
	rows, _ := db.ZdrTombstones().ListByOrg("org_a", 10)
	if len(rows) != 1 || rows[0].Outcome != "ok" {
		t.Fatalf("first write must win, got %+v", rows)
	}
}

func TestMemoryZdrTombstonePutRefusesAnInvalidTombstone(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	z := goodTombstone("run_1", "org_a", time.Now())
	z.Kind = "unknown"
	if _, err := db.ZdrTombstones().Put(z); err == nil {
		t.Fatal("the store must validate, not trust its caller")
	}
	if rows, _ := db.ZdrTombstones().ListByOrg("org_a", 10); len(rows) != 0 {
		t.Fatalf("an invalid tombstone was stored: %+v", rows)
	}
}

func TestMemoryZdrTombstoneDeleteBeforeIsBatchedAndOldestFirst(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	now := time.Now().UTC()
	for i, age := range []time.Duration{40, 35, 31, 29, 1} {
		z := goodTombstone("run_"+string(rune('a'+i)), "org_a", now.Add(-age*24*time.Hour))
		if _, err := db.ZdrTombstones().Put(z); err != nil {
			t.Fatal(err)
		}
	}
	cutoff := now.Add(-30 * 24 * time.Hour)
	if n, _ := db.ZdrTombstones().CountBefore(cutoff); n != 3 {
		t.Fatalf("CountBefore = %d, want 3", n)
	}
	n, err := db.ZdrTombstones().DeleteBefore(cutoff, 2)
	if err != nil || n != 2 {
		t.Fatalf("first batch: n=%d err=%v", n, err)
	}
	rows, _ := db.ZdrTombstones().ListByOrg("org_a", 10)
	if len(rows) != 3 {
		t.Fatalf("3 must remain after a batch of 2, got %d", len(rows))
	}
	// The oldest two went first, so the 31-day-old one is the only one left past the cutoff.
	if n, _ := db.ZdrTombstones().CountBefore(cutoff); n != 1 {
		t.Fatalf("one tombstone past the cutoff must remain, got %d", n)
	}
	if n, _ := db.ZdrTombstones().DeleteBefore(cutoff, 0); n != 0 {
		t.Fatalf("limit 0 must delete nothing, deleted %d", n)
	}
}

// WS4.J: org erasure includes the tombstones (the plan's TestOrgErasureRemovesTombstones).
func TestOrgErasureRemovesTombstones(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	now := time.Now().UTC()
	for _, z := range []ZdrTombstone{
		goodTombstone("run_a1", "org_a", now), goodTombstone("run_a2", "org_a", now),
		goodTombstone("run_b1", "org_b", now),
	} {
		if _, err := db.ZdrTombstones().Put(z); err != nil {
			t.Fatal(err)
		}
	}
	res, err := db.PurgeOrg("org_a")
	if err != nil {
		t.Fatal(err)
	}
	if res.ZdrTombstonesDeleted != 2 || res.Total() != 2 {
		t.Fatalf("PurgeOrg must report 2 tombstones, got %+v", res)
	}
	if rows, _ := db.ZdrTombstones().ListByOrg("org_a", 10); len(rows) != 0 {
		t.Fatalf("org_a tombstones survived erasure: %+v", rows)
	}
	if rows, _ := db.ZdrTombstones().ListByOrg("org_b", 10); len(rows) != 1 {
		t.Fatalf("org_b must keep its tombstone, got %+v", rows)
	}
	again, err := db.PurgeOrg("org_a")
	if err != nil || again.Total() != 0 {
		t.Fatalf("a repeat erasure must be a zero result, got %+v err=%v", again, err)
	}
}
