package database

import (
	"strings"
	"testing"
)

func squashSQL(sql string) string { return strings.Join(strings.Fields(sql), " ") }

func requireSQLFragments(t *testing.T, name, sql string, fragments []string) {
	t.Helper()
	squashed := squashSQL(sql)
	for _, fragment := range fragments {
		if !strings.Contains(squashed, squashSQL(strings.ToLower(fragment))) {
			t.Fatalf("migration %s missing %q", name, fragment)
		}
	}
}

// WS10-S2: migration 039 text contract (no database). Standing rules only,
// one live rule per agent action, an append-only event log of ids and codes,
// and a deny-by-default entitlement column.
func TestApprovalGrantsMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "039_approval_grants.up.sql")
	requireSQLFragments(t, "039", sql, []string{
		// The entitlement is its own bit, off by default.
		"alter table space_effect_policies add column if not exists approval_grant_entitled boolean not null default false",
		// Per-Space ceiling with the most restrictive defaults.
		"create table if not exists space_approval_policies",
		"max_grant_scope text not null default 'once'",
		"check (max_grant_scope in ('once', 'thread', 'always'))",
		"default_addressee_kind text not null default 'requester'",
		"check (default_addressee_kind in ('requester', 'any_editor', 'any_manager'))",
		"quarantine_mode text not null default 'off'",
		"check (quarantine_mode in ('off', 'shadow', 'enforce'))",
		"check (revision > 0)",
		// Standing rules are scope 'always' only.
		"create table if not exists space_approval_grants",
		"scope text not null check (scope = 'always')",
		"check (action_schema_hash ~ '^sha256:[0-9a-f]{64}$')",
		"check (granted_by_role in ('manager', 'owner'))",
		"check (authority_revision > 0)",
		"check (char_length(idempotency_key) between 8 and 200)",
		"unique (org_id, idempotency_key)",
		"references registered_spaces(space_ref) on delete restrict",
		// One live rule per agent action.
		"create unique index if not exists space_approval_grants_live_idx",
		"on space_approval_grants (space_ref, agent_subject_id, action_key)",
		"where revoked_at is null",
		// Event log vocabulary.
		"create table if not exists space_approval_grant_events",
		"check (kind in ('created', 'used', 'revoked', 'expired', 'use_denied'))",
		"create index if not exists space_approval_grant_events_rule_idx",
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
	// The log must not carry tool arguments or content.
	events := sql[strings.Index(sql, "create table if not exists space_approval_grant_events"):]
	for _, forbidden := range []string{"payload", "arguments", "content", "body", "jsonb"} {
		if strings.Contains(events, forbidden) {
			t.Fatalf("the approval event log must stay content-free, found %q", forbidden)
		}
	}

	down := readMigrationLower(t, "039_approval_grants.down.sql")
	requireSQLFragments(t, "039 down", down, []string{
		"drop table if exists space_approval_grant_events",
		"drop table if exists space_approval_grants",
		"drop table if exists space_approval_policies",
		"drop column if exists approval_grant_entitled",
	})
}
