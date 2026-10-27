package database

import (
	"strings"
	"testing"
)

// WS04-S7: migration 035 text contract. Archive is a Control registration
// state; a re-run must be harmless and the down migration must restore the
// original four-state CHECK after returning archived rooms to active.
func TestSpaceArchivedMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "035_space_archived_state.up.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	dropAt := strings.Index(flat, "drop constraint if exists registered_spaces_state_chk")
	addAt := strings.Index(flat, "add constraint registered_spaces_state_chk")
	if dropAt < 0 || addAt < 0 || dropAt > addAt {
		t.Fatal("registered_spaces_state_chk must be dropped (if exists) before it is re-added")
	}
	if !strings.Contains(flat, "check (registration_state in ('active', 'archived', 'suspended', 'deleting', 'deleted'))") {
		t.Fatal("the state CHECK must contain archived next to the four existing states")
	}
	if !strings.Contains(flat, "add column if not exists archived_at timestamptz") {
		t.Fatal("archived_at timestamptz column is missing or unguarded")
	}
	if !strings.Contains(flat, "add column if not exists archived_by text") {
		t.Fatal("archived_by text column is missing or unguarded")
	}
	if !strings.Contains(flat, "begin;") || !strings.Contains(flat, "commit;") {
		t.Fatal("migration 035 must run in one transaction")
	}
}

func TestSpaceArchivedMigrationDownRestoresTheFourStateCheck(t *testing.T) {
	sql := readMigrationLower(t, "035_space_archived_state.down.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	revive := strings.Index(flat, "update registered_spaces set registration_state = 'active' where registration_state = 'archived'")
	drop := strings.Index(flat, "drop constraint if exists registered_spaces_state_chk")
	if revive < 0 || drop < 0 || revive > drop {
		t.Fatal("archived rooms must be returned to active before the CHECK is narrowed")
	}
	if !strings.Contains(flat, "check (registration_state in ('active', 'suspended', 'deleting', 'deleted'))") {
		t.Fatal("down migration must restore the four-state CHECK")
	}
	for _, column := range []string{"archived_by", "archived_at"} {
		if !strings.Contains(flat, "drop column if exists "+column) {
			t.Fatalf("down migration must drop %s", column)
		}
	}
}
