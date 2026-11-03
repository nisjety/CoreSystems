package database

import (
	"strings"
	"testing"
)

// WS12-S1: migration 041 text contract (no database).
func TestSpaceDMKindsMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "041_space_dm_kinds.up.sql")
	requireSQLFragments(t, "041", sql, []string{
		"drop constraint if exists registered_spaces_kind_chk",
		"add constraint registered_spaces_kind_chk check (space_kind in ('personal', 'room', 'project', 'case', 'dm', 'group_dm'))",
		"add column if not exists participant_hash text",
		// A DM kind has a hash and no other kind does.
		"drop constraint if exists registered_spaces_dm_hash_chk",
		"check ((space_kind in ('dm', 'group_dm')) = (participant_hash is not null))",
		// One live DM per participant set per org.
		"create unique index if not exists uq_registered_spaces_dm_participants",
		"on registered_spaces (org_id, participant_hash)",
		"where participant_hash is not null and registration_state in ('active', 'suspended')",
		// Deny-by-default entitlement.
		"alter table space_effect_policies add column if not exists dm_create_entitled boolean not null default false",
	})
	if !strings.Contains(sql, "begin;") || !strings.Contains(sql, "commit;") {
		t.Fatal("migration 041 must run in one transaction")
	}
	// A pending or deleted DM must not block re-opening the same conversation.
	if strings.Contains(squashSQL(sql), "registration_state in ('active', 'suspended', 'archived'") {
		t.Fatal("archived DMs must not hold the participant-set slot")
	}

	down := readMigrationLower(t, "041_space_dm_kinds.down.sql")
	requireSQLFragments(t, "041 down", down, []string{
		"drop index if exists uq_registered_spaces_dm_participants",
		"drop column if exists dm_create_entitled",
		"drop column if exists participant_hash",
		"check (space_kind in ('personal', 'room', 'project', 'case'))",
	})
}
