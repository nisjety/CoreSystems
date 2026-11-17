package database

import (
	"strings"
	"testing"
)

// sqlStatements returns the migration's statements with "--" comments removed,
// whitespace squashed and empty statements dropped.
func sqlStatements(sql string) []string {
	lines := strings.Split(sql, "\n")
	for i, line := range lines {
		if at := strings.Index(line, "--"); at >= 0 {
			lines[i] = line[:at]
		}
	}
	var statements []string
	for _, statement := range strings.Split(squashSQL(strings.Join(lines, "\n")), ";") {
		if statement = strings.TrimSpace(statement); statement != "" {
			statements = append(statements, statement)
		}
	}
	return statements
}

// W3-DEC-CTL1: migration 049 text contract (no database). The per-turn
// personal-memory bit (WS17-S8, D12) is its own deny-by-default column: NOT NULL
// DEFAULT FALSE gives every existing org row FALSE, nothing backfills it (in
// particular not from memory_declassify_entitled, which D12 keeps separate), and
// the down file drops that one column.
func TestSpaceMemoryPersonalTurnMigrationContract(t *testing.T) {
	up := sqlStatements(readMigrationLower(t, "049_space_memory_personal_turn_entitlement.up.sql"))
	want := "alter table space_effect_policies add column if not exists memory_personal_turn_entitled boolean not null default false"
	if len(up) != 1 || up[0] != want {
		t.Fatalf("migration 049 up must be exactly %q, got %q", want, up)
	}

	down := sqlStatements(readMigrationLower(t, "049_space_memory_personal_turn_entitlement.down.sql"))
	want = "alter table space_effect_policies drop column if exists memory_personal_turn_entitled"
	if len(down) != 1 || down[0] != want {
		t.Fatalf("migration 049 down must be exactly %q, got %q", want, down)
	}
}
