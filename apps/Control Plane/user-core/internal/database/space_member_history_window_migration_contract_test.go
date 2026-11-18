package database

import (
	"strings"
	"testing"
)

// D18: migration 050 text contract. A per-member window is a mode and an instant and never content: its
// CHECKs keep since_at exactly when the mode is since_date, its rows go with the room, and the down
// migration removes the table.
func TestSpaceMemberHistoryWindowMigrationContract(t *testing.T) {
	sql := readMigrationLower(t, "050_space_member_history_window.up.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	if !strings.Contains(flat, "create table if not exists space_member_history_windows") {
		t.Fatal("the window table must be created idempotently")
	}
	if !strings.Contains(flat, "primary key (space_ref, subject_id)") {
		t.Fatal("one window per room member: the primary key must be (space_ref, subject_id)")
	}
	if !strings.Contains(flat, "references registered_spaces(space_ref) on delete cascade") {
		t.Fatal("a window must go with its room")
	}
	if !strings.Contains(flat, "check (mode in ('room', 'full', 'since_join', 'since_date'))") {
		t.Fatal("the mode CHECK must allow exactly room, full, since_join and since_date")
	}
	if !strings.Contains(flat, "check ((mode = 'since_date') = (since_at is not null))") {
		t.Fatal("since_at must be present exactly for since_date")
	}
	if !strings.Contains(flat, "revision bigint not null default 1") || !strings.Contains(flat, "check (revision > 0)") {
		t.Fatal("revision must be a positive BIGINT that starts at 1")
	}
	if !strings.Contains(flat, "begin;") || !strings.Contains(flat, "commit;") {
		t.Fatal("migration 050 must run in one transaction")
	}
	// Content-free by construction: the columns are ids, a mode, an instant, a revision and timestamps.
	// Only the statements count here, so the comment that explains the rule does not trip the check.
	var statements []string
	for _, line := range strings.Split(sql, "\n") {
		if !strings.HasPrefix(strings.TrimSpace(line), "--") {
			statements = append(statements, line)
		}
	}
	code := strings.Join(strings.Fields(strings.Join(statements, " ")), " ")
	for _, forbidden := range []string{"content", "body", "display_name", "email", "message_id"} {
		if strings.Contains(code, forbidden) {
			t.Fatalf("the window table must store no content, found %q", forbidden)
		}
	}
}

func TestSpaceMemberHistoryWindowMigrationDownDropsTheTable(t *testing.T) {
	sql := readMigrationLower(t, "050_space_member_history_window.down.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	if !strings.Contains(flat, "drop table if exists space_member_history_windows") {
		t.Fatal("the down migration must drop the window table")
	}
}
