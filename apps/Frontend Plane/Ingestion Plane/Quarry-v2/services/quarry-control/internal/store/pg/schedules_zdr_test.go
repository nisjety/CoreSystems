package pg

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// The migration text is what makes the stamp safe for existing rows; pin it
// without a database so the guarantee holds in every CI lane.
func TestMigration023Text(t *testing.T) {
	b, err := migrationFS.ReadFile("migrations/023_schedules_zdr.sql")
	if err != nil {
		t.Fatalf("read 023: %v", err)
	}
	var stmt strings.Builder
	for _, line := range strings.Split(string(b), "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		stmt.WriteString(line + " ")
	}
	got := strings.Join(strings.Fields(strings.ToUpper(stmt.String())), " ")
	if got != "ALTER TABLE SCHEDULES ADD COLUMN IF NOT EXISTS ZDR BOOLEAN;" {
		t.Fatalf("023 must be exactly one idempotent nullable ADD COLUMN, got %q", got)
	}
}

// WS4.H: the stamp round-trips through Postgres, and a row written before the
// column existed (zdr NULL) scans as nil, not false. Skips without a database.
func TestPgScheduleZdrRoundTrip(t *testing.T) {
	db := openOrSkip(t)
	yes, no := true, false
	mk := func(z *bool) store.Schedule {
		return store.Schedule{
			ID: quarrycontracts.NewID(quarrycontracts.KindSchedule), OrgID: "org_zdr_rt",
			Cron: "0 9 * * *", TargetKind: store.TargetKindChangeMonitor, TargetRef: "https://x.example/p",
			Enabled: true, CreatedAt: time.Now().UnixMilli(), Zdr: z,
		}
	}
	for _, in := range []*bool{&yes, &no, nil} {
		s := mk(in)
		if err := db.Schedules().Create(s); err != nil {
			t.Fatalf("create: %v", err)
		}
		got, ok := db.Schedules().Get(s.ID)
		if !ok {
			t.Fatalf("get %s: missing", s.ID)
		}
		if (got.Zdr == nil) != (in == nil) || (in != nil && *got.Zdr != *in) {
			t.Fatalf("round trip: got %v want %v", got.Zdr, in)
		}
		items, _ := db.Schedules().List(500, "")
		found := false
		for _, it := range items {
			if it.ID == s.ID {
				found = true
				if (it.Zdr == nil) != (in == nil) || (in != nil && *it.Zdr != *in) {
					t.Fatalf("list scan: got %v want %v", it.Zdr, in)
				}
			}
		}
		if !found {
			t.Fatalf("list did not return %s", s.ID)
		}
		_ = db.Schedules().Delete(s.ID)
	}
}

// Applying 023 a second time (a re-run, or a table WS15 rebuilt with the
// column) must be a no-op that keeps the stored values.
func TestMigration023IsIdempotent(t *testing.T) {
	db := openOrSkip(t)
	p, ok := db.(*postgresDB)
	if !ok {
		t.Skip("not a postgres DB")
	}
	b, err := migrationFS.ReadFile("migrations/023_schedules_zdr.sql")
	if err != nil {
		t.Fatal(err)
	}
	yes := true
	s := store.Schedule{
		ID: quarrycontracts.NewID(quarrycontracts.KindSchedule), OrgID: "org_zdr_idem",
		Cron: "0 9 * * *", TargetKind: store.TargetKindChangeMonitor, TargetRef: "https://x.example/i",
		Enabled: true, CreatedAt: time.Now().UnixMilli(), Zdr: &yes,
	}
	if err := db.Schedules().Create(s); err != nil {
		t.Fatalf("create: %v", err)
	}
	defer func() { _ = db.Schedules().Delete(s.ID) }()
	for i := 0; i < 2; i++ {
		if _, err := p.pool.Exec(context.Background(), string(b)); err != nil {
			t.Fatalf("re-apply %d: %v", i, err)
		}
	}
	got, ok := db.Schedules().Get(s.ID)
	if !ok || got.Zdr == nil || !*got.Zdr {
		t.Fatalf("stamp lost after re-apply: %v", got.Zdr)
	}
}
