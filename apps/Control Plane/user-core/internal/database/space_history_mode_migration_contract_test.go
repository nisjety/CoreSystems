package database

import (
	"strings"
	"testing"
)

// WS04-S11: migration 037 text contract. history_mode is a presentation rule on
// a room; a re-run must be harmless and the down migration must remove both
// the CHECK and the column.
func TestSpaceHistoryModeMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "037_space_history_mode.up.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	if !strings.Contains(flat, "add column if not exists history_mode text not null default 'full'") {
		t.Fatal("history_mode must be an idempotent NOT NULL TEXT column defaulting to full")
	}
	dropAt := strings.Index(flat, "drop constraint if exists registered_spaces_history_mode_chk")
	addAt := strings.Index(flat, "add constraint registered_spaces_history_mode_chk")
	if dropAt < 0 || addAt < 0 || dropAt > addAt {
		t.Fatal("the history_mode CHECK must be dropped (if exists) before it is re-added")
	}
	if !strings.Contains(flat, "check (history_mode in ('full', 'since_join'))") {
		t.Fatal("history_mode CHECK must allow exactly full and since_join")
	}
	if !strings.Contains(flat, "begin;") || !strings.Contains(flat, "commit;") {
		t.Fatal("migration 037 must run in one transaction")
	}
	// The floor is derived from audience snapshots, never stored.
	if strings.Contains(flat, "floor") && strings.Contains(flat, "add column if not exists history_floor") {
		t.Fatal("the floor must not be stored as a column")
	}
}

func TestSpaceHistoryModeMigrationDownReverses(t *testing.T) {
	sql := readMigrationLower(t, "037_space_history_mode.down.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	if !strings.Contains(flat, "drop constraint if exists registered_spaces_history_mode_chk") {
		t.Fatal("down migration must drop the CHECK")
	}
	if !strings.Contains(flat, "drop column if exists history_mode") {
		t.Fatal("down migration must drop the column")
	}
	if strings.Index(flat, "drop constraint") > strings.Index(flat, "drop column") {
		t.Fatal("the CHECK must be dropped before the column")
	}
}
