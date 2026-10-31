package pg

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// WS15.S1 (migration 017). The text pin runs without a database in every lane.
// The database tests skip without QUARRY_TEST_DSN and fail under QUARRY_REQUIRE_DB=1.

func TestMigration017Text(t *testing.T) {
	b, err := migrationFS.ReadFile("migrations/017_schedules_definition.sql")
	if err != nil {
		t.Fatalf("read 017: %v", err)
	}
	var stmt strings.Builder
	for _, line := range strings.Split(string(b), "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "--") {
			continue
		}
		stmt.WriteString(line + " ")
	}
	got := strings.Join(strings.Fields(strings.ToUpper(stmt.String())), " ")
	for _, want := range []string{
		"ALTER TABLE SCHEDULES ADD COLUMN IF NOT EXISTS NAME TEXT NOT NULL DEFAULT '';",
		"ALTER TABLE SCHEDULES ADD COLUMN IF NOT EXISTS CONFIG JSONB NOT NULL DEFAULT '{}';",
		"CREATE INDEX IF NOT EXISTS IDX_SCHEDULES_ORG_CREATED ON SCHEDULES(ORG_ID, CREATED_AT DESC);",
		"UPDATE SCHEDULES SET ENABLED = FALSE WHERE TARGET_KIND IN ('SCRAPE', 'CRAWL', 'BATCH') AND UPDATED_AT = 0;",
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("017 must contain %q", want)
		}
	}
	if strings.Contains(got, "CHANGE_MONITOR") {
		t.Fatal("017 must not touch change_monitor rows")
	}
}

// rerunMigration017 executes the file again, as a manual re-run would.
func rerunMigration017(t *testing.T, p *postgresDB) {
	t.Helper()
	sqlText, err := migrationFS.ReadFile("migrations/017_schedules_definition.sql")
	if err != nil {
		t.Fatalf("read 017: %v", err)
	}
	if _, err := p.pool.Exec(context.Background(), string(sqlText)); err != nil {
		t.Fatalf("re-run 017: %v", err)
	}
}

// insertPre017Row writes a row the way pre-017 code did: no updated_at stamp.
func insertPre017Row(t *testing.T, p *postgresDB, org, kind string, enabled bool) quarrycontracts.ID {
	t.Helper()
	id := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	if _, err := p.pool.Exec(context.Background(),
		`INSERT INTO schedules(id, org_id, cron, target_kind, target_ref, enabled, created_at, created_by, updated_at)
		 VALUES ($1,$2,'0 7 * * *',$3,'https://x.example',$4,$5,'',0)`,
		string(id), org, kind, enabled, time.Now().Unix()); err != nil {
		t.Fatalf("insert pre-017 %s row: %v", kind, err)
	}
	t.Cleanup(func() {
		_, _ = p.pool.Exec(context.Background(), `DELETE FROM schedules WHERE id=$1`, string(id))
	})
	return id
}

func enabledOf(t *testing.T, p *postgresDB, id quarrycontracts.ID) bool {
	t.Helper()
	var enabled bool
	if err := p.pool.QueryRow(context.Background(),
		`SELECT enabled FROM schedules WHERE id=$1`, string(id)).Scan(&enabled); err != nil {
		t.Fatalf("read enabled for %s: %v", id, err)
	}
	return enabled
}

func TestMigration017_DisablesPreexistingNonMonitorRows(t *testing.T) {
	db := openOrSkip(t)
	p := db.(*postgresDB)
	org := "org_m017_" + string(quarrycontracts.NewID(quarrycontracts.KindSchedule))

	scrape := insertPre017Row(t, p, org, store.TargetKindScrape, true)
	crawl := insertPre017Row(t, p, org, store.TargetKindCrawl, true)
	monitor := insertPre017Row(t, p, org, store.TargetKindChangeMonitor, true)

	rerunMigration017(t, p)

	if enabledOf(t, p, scrape) || enabledOf(t, p, crawl) {
		t.Fatal("pre-017 scrape and crawl rows must be disabled by the guard")
	}
	if !enabledOf(t, p, monitor) {
		t.Fatal("pre-017 change_monitor rows must be left alone")
	}
}

func TestMigration017_RerunKeepsReenabledRow(t *testing.T) {
	db := openOrSkip(t)
	p := db.(*postgresDB)
	org := "org_m017r_" + string(quarrycontracts.NewID(quarrycontracts.KindSchedule))

	id := insertPre017Row(t, p, org, store.TargetKindScrape, true)
	rerunMigration017(t, p)
	if enabledOf(t, p, id) {
		t.Fatal("the first run must disable the pre-017 scrape row")
	}
	// An operator re-enables it. UpdateEnabled stamps updated_at, so the guard
	// (updated_at = 0) no longer matches this row.
	if err := db.Schedules().UpdateEnabled(id, true); err != nil {
		t.Fatalf("re-enable: %v", err)
	}
	rerunMigration017(t, p)
	if !enabledOf(t, p, id) {
		t.Fatal("a re-run must not disable a row an operator re-enabled")
	}
}

// TestPgScheduleOrgScopeAndConfigRoundTrip: the definition round-trips through
// JSONB, and every tenant method answers a foreign org exactly as a missing row.
func TestPgScheduleOrgScopeAndConfigRoundTrip(t *testing.T) {
	db := openOrSkip(t)
	org := "org_s1_" + string(quarrycontracts.NewID(quarrycontracts.KindSchedule))
	other := org + "_other"
	now := time.Now().Unix()
	s := store.Schedule{
		ID:            quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:         org,
		Name:          "Nightly crawl",
		Cron:          "0 7 * * *",
		TargetKind:    store.TargetKindCrawl,
		TargetRef:     "https://x.example/docs",
		Timezone:      "Europe/Oslo",
		OverlapPolicy: "skip",
		Config:        map[string]any{"url": "https://x.example/docs", "max_pages": float64(20)},
		Enabled:       false,
		CreatedAt:     now,
		UpdatedAt:     now,
	}
	if err := db.Schedules().Create(s); err != nil {
		t.Fatalf("create: %v", err)
	}
	t.Cleanup(func() { _ = db.Schedules().Delete(s.ID) })

	got, ok := db.Schedules().GetByOrg(s.ID, org)
	if !ok {
		t.Fatal("GetByOrg for the owning org must find the row")
	}
	if got.Name != "Nightly crawl" || got.Timezone != "Europe/Oslo" || got.Config["max_pages"] != float64(20) {
		t.Fatalf("definition did not round-trip: %+v", got)
	}
	if _, ok := db.Schedules().GetByOrg(s.ID, other); ok {
		t.Fatal("GetByOrg for a foreign org must not find the row")
	}
	if items, _ := db.Schedules().ListByOrg(other, 50, ""); len(items) != 0 {
		t.Fatalf("ListByOrg(other) returned %d rows, want 0", len(items))
	}
	if items, _ := db.Schedules().ListByOrg(org, 50, ""); len(items) != 1 {
		t.Fatalf("ListByOrg(org) returned %d rows, want 1", len(items))
	}
	if err := db.Schedules().UpdateEnabledByOrg(s.ID, other, true); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("foreign UpdateEnabledByOrg: %v, want ErrNotFound", err)
	}
	if err := db.Schedules().DeleteByOrg(s.ID, other); !errors.Is(err, store.ErrNotFound) {
		t.Fatalf("foreign DeleteByOrg: %v, want ErrNotFound", err)
	}
	if err := db.Schedules().UpdateEnabledByOrg(s.ID, org, true); err != nil {
		t.Fatalf("own UpdateEnabledByOrg: %v", err)
	}
}
