package pg

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// E3.5-6 (migration 027). The text pin runs without a database in every lane.
// The database tests skip without QUARRY_TEST_DSN and fail under QUARRY_REQUIRE_DB=1,
// as the other Postgres tests do.

// TestMigration027Text pins the shape of migration 027: both tables gain the
// same untagged-by-default column, both org-scoped partial indexes exist, and
// the migration rewrites or drops nothing that already exists.
func TestMigration027Text(t *testing.T) {
	b, err := migrationFS.ReadFile("migrations/027_monitor_tags.sql")
	if err != nil {
		t.Fatalf("read 027: %v", err)
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
		"ALTER TABLE SCHEDULES ADD COLUMN IF NOT EXISTS TAG TEXT NOT NULL DEFAULT '';",
		"ALTER TABLE QUARRY_SOURCES ADD COLUMN IF NOT EXISTS TAG TEXT NOT NULL DEFAULT '';",
		"CREATE INDEX IF NOT EXISTS SCHEDULES_ORG_TAG_CREATED_IDX ON SCHEDULES (ORG_ID, TAG, CREATED_AT DESC) WHERE TAG <> '';",
		"CREATE INDEX IF NOT EXISTS QUARRY_SOURCES_ORG_TAG_CREATED_IDX ON QUARRY_SOURCES (ORG_ID, TAG, CREATED_AT DESC) WHERE TAG <> '' AND DELETED_AT IS NULL;",
	} {
		if !strings.Contains(got, want) {
			t.Fatalf("027 must contain %q", want)
		}
	}
	for _, banned := range []string{"DROP ", "DELETE ", "UPDATE ", "RENAME ", "ALTER COLUMN", "TRUNCATE "} {
		if strings.Contains(got, banned) {
			t.Fatalf("027 is additive only, but it contains %q", banned)
		}
	}
}

// TestPostgres_MonitorTagsMigrationReRunIsNoOp runs 027 a second time on a migrated
// schema, as a manual re-run would. It must succeed and change nothing.
func TestPostgres_MonitorTagsMigrationReRunIsNoOp(t *testing.T) {
	p := openIsolatedOrSkip(t).(*postgresDB)
	sqlText, err := migrationFS.ReadFile("migrations/027_monitor_tags.sql")
	if err != nil {
		t.Fatalf("read 027: %v", err)
	}
	if _, err := p.pool.Exec(context.Background(), string(sqlText)); err != nil {
		t.Fatalf("re-run 027: %v", err)
	}
}

// newTaggedMonitor is a change monitor of org carrying tag ("" is untagged).
func newTaggedMonitor(org, tag string) store.Schedule {
	return store.Schedule{
		ID:         quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:      org,
		Cron:       "0 * * * *",
		TargetKind: store.TargetKindChangeMonitor,
		TargetRef:  "https://" + org + ".example/pricing",
		Enabled:    true,
		CreatedAt:  time.Now().Unix(),
		Tag:        tag,
	}
}

// newTaggedSource is a tracked source of org at url carrying tag.
func newTaggedSource(org, url, tag string) store.Source {
	now := time.Now().UnixMilli()
	return store.Source{
		ID:        quarrycontracts.NewID(quarrycontracts.KindSource),
		OrgID:     org,
		Name:      url,
		URL:       url,
		Kind:      "scrape",
		Status:    "active",
		CreatedAt: now,
		UpdatedAt: now,
		Tag:       tag,
	}
}

// TestPostgres_MonitorTagsAreOrgScoped proves the tag contract against the migrated
// schema: a tag is stored in its normalised form, the tag lists are org-scoped, a
// cross-tenant retag answers ErrNotFound, and a repeat source registration keeps the
// tag the row already has.
func TestPostgres_MonitorTagsAreOrgScoped(t *testing.T) {
	db := openIsolatedOrSkip(t)

	a := newTaggedMonitor("org_a", "Pricing")
	b := newTaggedMonitor("org_b", "pricing")
	for _, sch := range []store.Schedule{a, b} {
		if err := db.Schedules().Create(sch); err != nil {
			t.Fatalf("create schedule %s: %v", sch.ID, err)
		}
	}
	if got, ok := db.Schedules().GetByOrg(a.ID, "org_a"); !ok || got.Tag != "pricing" {
		t.Fatalf("stored schedule tag = %q (found %v); want the normalised pricing", got.Tag, ok)
	}
	if got, _ := db.Schedules().ListByOrgTag("org_a", "PRICING", 50, ""); len(got) != 1 || got[0].ID != a.ID {
		t.Fatalf("org_a schedule tag list has %d rows; want only a", len(got))
	}
	if got, _ := db.Schedules().ListByOrgTag("org_b", "pricing", 50, ""); len(got) != 1 || got[0].ID != b.ID {
		t.Fatalf("org_b schedule tag list has %d rows; want only b", len(got))
	}
	if err := db.Schedules().UpdateTagByOrg(b.ID, "org_a", "ops"); err != store.ErrNotFound {
		t.Fatalf("cross-tenant schedule retag err = %v; want ErrNotFound", err)
	}
	if got, _ := db.Schedules().GetByOrg(b.ID, "org_b"); got.Tag != "pricing" {
		t.Fatalf("org_b schedule tag after a refused retag = %q; want pricing", got.Tag)
	}
	if err := db.Schedules().UpdateTagByOrg(a.ID, "org_a", "not a tag"); err == nil {
		t.Fatal("an invalid schedule tag was stored")
	}

	src := newTaggedSource("org_a", "https://a.example/pricing", "pricing")
	stored, created, err := db.Sources().UpsertByOrgAndURL(src)
	if err != nil || !created || stored.Tag != "pricing" {
		t.Fatalf("upsert source = %+v created=%v err=%v; want tag pricing, created", stored, created, err)
	}
	again := newTaggedSource("org_a", "https://a.example/pricing", "ops")
	kept, created2, err := db.Sources().UpsertByOrgAndURL(again)
	if err != nil || created2 || kept.ID != stored.ID || kept.Tag != "pricing" {
		t.Fatalf("repeat upsert = %+v created=%v err=%v; want the first row with tag pricing", kept, created2, err)
	}
	if got, _ := db.Sources().ListByOrgTag("org_a", "pricing", 50, ""); len(got) != 1 || got[0].ID != stored.ID {
		t.Fatalf("org_a source tag list has %d rows; want only the source", len(got))
	}
	if got, _ := db.Sources().ListByOrgTag("org_b", "pricing", 50, ""); len(got) != 0 {
		t.Fatalf("org_b sees org_a's source: %d rows", len(got))
	}
	if err := db.Sources().UpdateTagByOrg("org_b", stored.ID, "ops"); err != store.ErrNotFound {
		t.Fatalf("cross-tenant source retag err = %v; want ErrNotFound", err)
	}
	if err := db.Sources().UpdateTagByOrg("org_a", stored.ID, "ops"); err != nil {
		t.Fatalf("owner source retag: %v", err)
	}
	if got, _ := db.Sources().GetByOrg("org_a", stored.ID); got.Tag != "ops" {
		t.Fatalf("source tag = %q; want ops", got.Tag)
	}
}
