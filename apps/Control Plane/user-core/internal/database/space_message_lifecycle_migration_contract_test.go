package database

import (
	"strings"
	"testing"
)

// WS12-S5: migration 042 text contract. Two deny-by-default entitlement
// columns, a closed original-text rule with no default row (absence means
// purge), and an operator evidence table.
func TestSpaceMessageLifecycleMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "042_space_message_lifecycle.up.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	for _, want := range []string{
		"add column if not exists message_revise_entitled boolean not null default false",
		"add column if not exists message_moderate_entitled boolean not null default false",
		"create table if not exists space_message_policies",
		"org_id text primary key",
		"create table if not exists space_message_policy_events",
		"event_id bigserial primary key",
		"create index if not exists idx_space_message_policy_events_org",
		"begin;", "commit;",
	} {
		if !strings.Contains(flat, want) {
			t.Fatalf("migration 042 must contain %q", want)
		}
	}
	if got := strings.Count(flat, "check (original_retention in ('retain', 'purge'))"); got != 2 {
		t.Fatalf("both tables must CHECK original_retention to retain|purge, found %d", got)
	}
	if strings.Contains(flat, "insert into space_message_policies") {
		t.Fatal("there must be no default policy row: absence means purge")
	}
	if strings.Contains(flat, "default 'retain'") || strings.Contains(flat, "default 'purge'") {
		t.Fatal("original_retention has no column default; the resolver supplies purge")
	}
}

func TestSpaceMessageLifecycleMigrationDownIsIdempotent(t *testing.T) {
	// The down file must only drop with IF EXISTS so a re-run is harmless.
	sql := readMigrationLower(t, "042_space_message_lifecycle.down.sql")
	flat := strings.Join(strings.Fields(sql), " ")
	for _, stmt := range strings.Split(flat, ";") {
		stmt = strings.TrimSpace(stmt)
		if strings.HasPrefix(stmt, "drop ") && !strings.Contains(stmt, "if exists") {
			t.Fatalf("down statement without IF EXISTS: %q", stmt)
		}
	}
}
