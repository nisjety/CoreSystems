package database

import (
	"strings"
	"testing"
)

// WS04-S8: migration 036 text contract. Visibility and join role are CHECKed
// columns, the directory policy has NO default row (absence denies), and the
// down migration drops in reverse dependency order.
func TestSpaceDirectoryMigrationUpContract(t *testing.T) {
	sql := readMigrationLower(t, "036_space_visibility_directory.up.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	if !strings.Contains(flat, "begin;") || !strings.Contains(flat, "commit;") {
		t.Fatal("migration 036 must run in one transaction")
	}
	for _, column := range []string{
		"add column if not exists visibility text not null default 'private'",
		"add column if not exists join_role text not null default 'editor'",
	} {
		if !strings.Contains(flat, column) {
			t.Errorf("missing guarded column: %s", column)
		}
	}

	// Each CHECK is dropped (if exists) before it is re-added, so a re-run is harmless.
	for _, constraint := range []string{"registered_spaces_visibility_chk", "registered_spaces_join_role_chk"} {
		drop := strings.Index(flat, "drop constraint if exists "+constraint)
		add := strings.Index(flat, "add constraint "+constraint)
		if drop < 0 || add < 0 || drop > add {
			t.Errorf("%s must be dropped (if exists) before it is re-added", constraint)
		}
	}
	if !strings.Contains(flat, "check (visibility in ('private', 'org_public'))") {
		t.Error("visibility CHECK must be exactly private and org_public")
	}
	if !strings.Contains(flat, "check (join_role in ('viewer', 'editor'))") {
		t.Error("join_role CHECK must be exactly viewer and editor")
	}
	joinCheck := flat[strings.Index(flat, "add constraint registered_spaces_join_role_chk"):]
	joinCheck = joinCheck[:strings.Index(joinCheck, ";")]
	for _, elevated := range []string{"manager", "owner"} {
		if strings.Contains(joinCheck, elevated) {
			t.Errorf("a self-join must never be able to receive %s", elevated)
		}
	}

	// Deny by default: the table exists, the flag defaults off, and no row is seeded.
	if !strings.Contains(flat, "create table if not exists space_directory_policies") ||
		!strings.Contains(flat, "org_public_rooms_enabled boolean not null default false") {
		t.Error("space_directory_policies must default the switch to false")
	}
	if strings.Contains(flat, "insert into") {
		t.Error("036 must not insert any row: an absent policy row denies")
	}
	if !strings.Contains(flat, "create table if not exists space_directory_policy_events") {
		t.Error("the policy change ledger table is missing")
	}

	// The directory index is partial on exactly what a listed room is.
	if !strings.Contains(flat, "create index if not exists idx_registered_spaces_directory on registered_spaces (org_id) where visibility = 'org_public' and registration_state = 'active'") {
		t.Errorf("partial directory index is missing or wrong: %s", flat)
	}
}

func TestSpaceDirectoryMigrationDownDropsInReverseOrder(t *testing.T) {
	sql := readMigrationLower(t, "036_space_visibility_directory.down.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	order := []string{
		"drop index if exists idx_registered_spaces_directory",
		"drop table if exists space_directory_policy_events",
		"drop table if exists space_directory_policies",
		"drop constraint if exists registered_spaces_join_role_chk",
		"drop constraint if exists registered_spaces_visibility_chk",
		"drop column if exists join_role",
		"drop column if exists visibility",
	}
	last := -1
	for _, statement := range order {
		at := strings.Index(flat, statement)
		if at < 0 {
			t.Fatalf("down migration lacks: %s", statement)
		}
		if at < last {
			t.Fatalf("%q is out of order: the index and tables go before the constraints, the constraints before their columns", statement)
		}
		last = at
	}
}
