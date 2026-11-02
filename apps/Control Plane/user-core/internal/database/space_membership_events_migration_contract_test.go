package database

import (
	"os"
	"strings"
	"testing"
)

func readMigrationLower(t *testing.T, name string) string {
	t.Helper()
	contents, err := os.ReadFile("../../migrations/" + name)
	if err != nil {
		t.Fatal(err)
	}
	return strings.ToLower(strings.ReplaceAll(string(contents), "\r\n", "\n"))
}

// WS04-S4: migration 034 text contract. A re-run must be harmless, the two
// closed vocabularies are CHECK-enforced, the idempotency key is unique per
// Space, and the ledger carries ids and roles only.
func TestSpaceMembershipEventsMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "034_space_membership_source_and_events.up.sql")

	for _, required := range []string{
		"add column if not exists source text not null default 'application_sync'",
		"check (source in ('registration', 'application_sync', 'member_ops', 'self_join'))",
		"add column if not exists roster_mode text not null default 'managed'",
		"check (roster_mode in ('managed', 'org_derived'))",
		"create table if not exists space_membership_events",
		"create unique index if not exists idx_space_membership_events_idem",
		"on space_membership_events (space_ref, idempotency_key)",
		"where idempotency_key is not null",
		"create index if not exists idx_space_membership_events_pending",
		"on space_membership_events (next_publish_at)",
		"where published_at is null",
		"create index if not exists idx_space_membership_events_space",
		"actor_id         text not null",
	} {
		if !strings.Contains(strings.Join(strings.Fields(sql), " "), strings.Join(strings.Fields(required), " ")) {
			t.Fatalf("migration 034 missing %q", required)
		}
	}

	// Idempotent: each constraint is dropped before it is added, and every
	// create statement is guarded.
	for _, constraint := range []string{"space_memberships_source_chk", "registered_spaces_roster_mode_chk"} {
		drop := "drop constraint if exists " + constraint
		add := "add constraint " + constraint
		dropAt, addAt := strings.Index(sql, drop), strings.Index(sql, add)
		if dropAt < 0 || addAt < 0 || dropAt > addAt {
			t.Fatalf("constraint %s must be dropped (if exists) before it is added", constraint)
		}
	}
	for _, line := range strings.Split(sql, "\n") {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "create table ") && !strings.HasPrefix(trimmed, "create table if not exists ") {
			t.Fatalf("unguarded create table: %q", trimmed)
		}
		if strings.HasPrefix(trimmed, "create index ") || strings.HasPrefix(trimmed, "create unique index ") {
			if !strings.Contains(trimmed, "if not exists") {
				t.Fatalf("unguarded create index: %q", trimmed)
			}
		}
	}
	// Registrar rows are re-labelled so the declarative sync never touches them.
	if !strings.Contains(sql, "set source = 'registration'") || !strings.Contains(sql, "granted_by = 'application-space-registration'") {
		t.Fatal("migration 034 must back-fill registrar rows with source registration")
	}
	if !strings.Contains(sql, "begin;") || !strings.Contains(sql, "commit;") {
		t.Fatal("migration 034 must run in one transaction")
	}

	// The ledger holds ids, roles and counts only.
	for _, forbidden := range []string{"display_name", "email", "body_text", "message_content", "service_token"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("membership ledger must not carry %q", forbidden)
		}
	}
}

func TestSpaceMembershipEventsMigrationDownReverts(t *testing.T) {
	sql := readMigrationLower(t, "034_space_membership_source_and_events.down.sql")
	for _, required := range []string{
		"drop table if exists space_membership_events",
		"drop column if exists roster_mode",
		"drop column if exists source",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration 034 down missing %q", required)
		}
	}
}
