package store

import (
	"os"
	"strings"
	"testing"
)

// WS05-S4: migration 011 is registered in order, additive and re-appliable.

func TestMigration011AuditReadIndexesIsRegisteredLast(t *testing.T) {
	last := migrations[len(migrations)-1]
	if last.version != "011_audit_read_indexes" {
		t.Fatalf("last migration = %q, want 011_audit_read_indexes", last.version)
	}
	seen := map[string]bool{}
	for _, m := range migrations {
		if seen[m.version] {
			t.Fatalf("duplicate migration %s", m.version)
		}
		seen[m.version] = true
		if strings.TrimSpace(m.sql) == "" {
			t.Fatalf("migration %s embeds empty SQL", m.version)
		}
	}
	if !strings.Contains(last.sql, "idx_audit_events_org_ts_id") ||
		!strings.Contains(last.sql, "idx_audit_events_org_user_ts") ||
		!strings.Contains(last.sql, "idx_audit_events_org_plane_ts") {
		t.Fatal("011 must create the three read indexes")
	}
}

func TestMigration011IsAdditiveAndIdempotentSQL(t *testing.T) {
	sql := strings.ToUpper(migrations[len(migrations)-1].sql)
	if strings.Count(sql, "CREATE INDEX") != strings.Count(sql, "CREATE INDEX IF NOT EXISTS") {
		t.Fatal("every index in 011 needs IF NOT EXISTS so a re-apply is safe")
	}
	for _, forbidden := range []string{"DROP ", "ALTER TABLE", "DELETE ", "TRUNCATE", "UPDATE "} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("011 must be purely additive, found %q", forbidden)
		}
	}
}

func TestMigration011RevertDropsExactlyTheCreatedIndexes(t *testing.T) {
	raw, err := os.ReadFile("migrations/revert/011_audit_read_indexes.down.sql")
	if err != nil {
		t.Fatal(err)
	}
	down := string(raw)
	for _, index := range []string{"idx_audit_events_org_ts_id", "idx_audit_events_org_user_ts", "idx_audit_events_org_plane_ts"} {
		if !strings.Contains(down, "DROP INDEX IF EXISTS "+index) {
			t.Fatalf("revert must drop %s", index)
		}
	}
	if strings.Contains(strings.ToUpper(down), "DROP TABLE") {
		t.Fatal("revert must not drop tables")
	}
}
