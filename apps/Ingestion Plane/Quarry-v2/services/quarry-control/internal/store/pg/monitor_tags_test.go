package pg

import (
	"strings"
	"testing"
)

// E3.5-6 (migration 027). The text pin runs without a database in every lane.
// The database tests in this package for tags skip without QUARRY_TEST_DSN and
// fail under QUARRY_REQUIRE_DB=1, as the other Postgres tests do.

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
