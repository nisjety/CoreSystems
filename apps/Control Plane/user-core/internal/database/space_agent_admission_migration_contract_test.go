package database

import (
	"strings"
	"testing"
)

// WS17-S4: migration 045 text contract (no database). Control's registry of
// agent subjects, their access list and per-Space admission, plus the two
// entitlement floors (personal_agent_admit_entitled, memory_declassify_entitled)
// plan 4.6 reserved for the 045 slot. readMigrationLower, requireSQLFragments and
// squashSQL are shared helpers (approval_grants_migration_contract_test.go).
func TestSpaceAgentAdmissionMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "045_agent_subjects_and_admissions.up.sql")
	requireSQLFragments(t, "045", sql, []string{
		"create table if not exists agent_subjects",
		"constraint agent_subjects_id_chk check (agent_subject_id ~ '^agent-[a-za-z0-9_-]{6,64}$')",
		"constraint agent_subjects_owner_chk check (owner_user_id is null or char_length(btrim(owner_user_id)) > 0)",
		"constraint agent_subjects_visibility_chk check (visibility in ('owner', 'org'))",
		"constraint agent_subjects_owner_required_chk check (visibility <> 'owner' or owner_user_id is not null)",
		"constraint agent_subjects_add_policy_chk check (add_policy in ('owner_only', 'nobody', 'anyone'))",
		// "anyone" is only ever valid for an org-visible agent.
		"constraint agent_subjects_policy_chk check (add_policy <> 'anyone' or visibility = 'org')",
		"constraint agent_subjects_lifecycle_chk check (lifecycle in ('active', 'suspended', 'retired'))",
		"constraint agent_subjects_revision_chk check (revision > 0)",
		"create unique index if not exists agent_subjects_subject_uq on agent_subjects (agent_subject_id)",

		"create table if not exists agent_subject_access",
		"constraint agent_subject_access_principal_chk check (principal_type = 'user')",
		"constraint agent_subject_access_role_chk check (role in ('manager', 'collaborator'))",
		"references agent_subjects (org_id, agent_subject_id) on delete cascade",

		"create table if not exists space_agent_admissions",
		"references registered_spaces (space_ref) on delete cascade",
		"constraint space_agent_admissions_state_chk check (state in ('admitted', 'withdrawn', 'suspended'))",
		"constraint space_agent_admissions_revision_chk check (revision > 0)",

		// Append-only ledger: ids, states and the idempotency key only.
		"create table if not exists space_agent_admission_events",
		"constraint space_agent_admission_events_event_chk",
		"check (event in ('admitted', 'withdrawn', 'suspended', 'would_skip'))",
		"create unique index if not exists space_agent_admission_events_key_uq",
		"on space_agent_admission_events (space_ref, idempotency_key) where idempotency_key <> ''",

		// The two deny-by-default entitlement floors plan 4.6 reserved for this slot.
		"alter table space_effect_policies",
		"add column if not exists personal_agent_admit_entitled boolean not null default false",
		"add column if not exists memory_declassify_entitled boolean not null default false",
	})
	for _, line := range strings.Split(sql, "\n") {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "create table ") && !strings.HasPrefix(trimmed, "create table if not exists ") {
			t.Fatalf("unguarded create table: %q", trimmed)
		}
		if (strings.HasPrefix(trimmed, "create index ") || strings.HasPrefix(trimmed, "create unique index ")) && !strings.Contains(trimmed, "if not exists") {
			t.Fatalf("unguarded create index: %q", trimmed)
		}
	}
	if !strings.Contains(sql, "begin;") || !strings.Contains(sql, "commit;") {
		t.Fatal("migration 045 must run in one transaction")
	}

	// The admission ledger is append-only: a trigger rejects UPDATE and DELETE.
	at := strings.Index(sql, "create table if not exists space_agent_admission_events")
	if at < 0 {
		t.Fatal("space_agent_admission_events table not found")
	}
	events := sql[at:]
	if !strings.Contains(events, "before update or delete on space_agent_admission_events") {
		t.Fatal("space_agent_admission_events must reject UPDATE and DELETE (no immutability trigger found)")
	}
	if !strings.Contains(events, "raise exception") {
		t.Fatal("the append-only trigger must refuse with an exception")
	}

	// Legacy backfill: idempotent (ON CONFLICT DO NOTHING on both inserts), and
	// an id that does not match the agent pattern is never backfilled.
	requireSQLFragments(t, "045 backfill", sql, []string{
		"insert into agent_subjects",
		"on conflict do nothing",
		"insert into space_agent_admissions",
		"on conflict (space_ref, agent_subject_id) do nothing",
		"and m.subject_id ~ '^agent-[a-za-z0-9_-]{6,64}$'",
	})
	// Ids, states and revisions only -- no agent name, instructions or content
	// (sqlStatements strips "--" comments, so the design-note prose above does
	// not trip this check on its own description of what the table must not hold).
	commentFree := strings.Join(sqlStatements(sql), " ")
	for _, forbidden := range []string{"display_name", "system_prompt", "body_text", "email"} {
		if strings.Contains(commentFree, forbidden) {
			t.Fatalf("the agent registry migration must stay content-free, found %q", forbidden)
		}
	}

	down := readMigrationLower(t, "045_agent_subjects_and_admissions.down.sql")
	requireSQLFragments(t, "045 down", down, []string{
		"alter table space_effect_policies",
		"drop column if exists memory_declassify_entitled",
		"drop column if exists personal_agent_admit_entitled",
		"drop trigger if exists space_agent_admission_events_immutable_trg on space_agent_admission_events",
		"drop function if exists space_agent_admission_events_immutable()",
		"drop table if exists space_agent_admission_events",
		"drop table if exists space_agent_admissions",
		"drop table if exists agent_subject_access",
		"drop table if exists agent_subjects",
	})
}
