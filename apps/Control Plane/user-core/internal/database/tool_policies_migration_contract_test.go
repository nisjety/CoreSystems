package database

import (
	"strings"
	"testing"
)

// WS10-S5a: migration 040 text contract. Two layers only (space_ceiling and
// agent), three levels, one row per tool key per layer, and a per-layer
// revision row that survives an emptied layer.
func TestSpaceToolPoliciesMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "040_space_tool_policies.up.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	for _, want := range []string{
		"create table if not exists space_tool_policy_sets",
		"create table if not exists space_tool_policies",
		"create unique index if not exists space_tool_policies_key_idx",
	} {
		if !strings.Contains(flat, want) {
			t.Fatalf("migration 040 must contain %q", want)
		}
	}
	// Layers are closed on both tables.
	if got := strings.Count(flat, "check (layer in ('space_ceiling', 'agent'))"); got != 2 {
		t.Fatalf("both tables must CHECK the layer to space_ceiling|agent, found %d", got)
	}
	if strings.Contains(flat, "'organization'") || strings.Contains(flat, "'org_floor'") {
		t.Fatal("the organisation floor layer is not stored by Control")
	}
	// Levels are closed.
	if !strings.Contains(flat, "check (level in ('allow', 'ask', 'block'))") {
		t.Fatal("level must be CHECKed to allow|ask|block")
	}
	// Tool key bounds.
	if !strings.Contains(flat, "check (char_length(btrim(tool_key)) between 1 and 200)") {
		t.Fatal("tool_key must be 1..200 characters after trimming")
	}
	// The unique key covers the space, layer, agent (coalesced) and tool key.
	idx := flat[strings.Index(flat, "create unique index if not exists space_tool_policies_key_idx"):]
	if !strings.Contains(idx, "(space_ref, layer, coalesce(agent_subject_id, ''), tool_key)") {
		t.Fatalf("unique key index must cover space, layer, agent and tool key: %s", idx)
	}
	// Layer/agent coupling on the entry table and the set table.
	if !strings.Contains(flat, "(layer = 'space_ceiling' and agent_subject_id is null) or (layer = 'agent' and agent_subject_id is not null") {
		t.Fatal("entry rows: the ceiling names no agent and the agent layer must")
	}
	if !strings.Contains(flat, "(layer = 'space_ceiling' and agent_key = '') or (layer = 'agent' and char_length(btrim(agent_key)) > 0)") {
		t.Fatal("set rows: the ceiling has an empty agent_key and the agent layer a non-blank one")
	}
	// The set row keys one revision per (space, layer, agent) and never reaches zero.
	if !strings.Contains(flat, "primary key (space_ref, layer, agent_key)") || !strings.Contains(flat, "revision bigint not null check (revision > 0)") {
		t.Fatal("set table needs a (space_ref, layer, agent_key) key and a positive revision")
	}
	// Rows must not be able to vanish with the room silently.
	if got := strings.Count(flat, "references registered_spaces(space_ref) on delete restrict"); got != 2 {
		t.Fatalf("both tables must restrict deletion of the Space, found %d", got)
	}
}

func TestSpaceToolPoliciesMigrationDownDropsBothTables(t *testing.T) {
	sql := readMigrationLower(t, "040_space_tool_policies.down.sql")
	flat := strings.Join(strings.Fields(sql), " ")
	if !strings.Contains(flat, "drop table if exists space_tool_policies;") || !strings.Contains(flat, "drop table if exists space_tool_policy_sets;") {
		t.Fatal("down migration must drop both tables idempotently")
	}
}
