package database

import (
	"strings"
	"testing"
)

// WS14-S2: migration 043 text contract. A Space becomes a legal grant subject,
// view-only; the down migration must delete the Space rows before it narrows
// the CHECK back, or it would fail on any database that holds one.
func TestSpaceSubjectGrantMigrationUpContract(t *testing.T) {
	sql := readMigrationLower(t, "043_resource_grants_space_subject.up.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	dropAt := strings.Index(flat, "drop constraint if exists resource_grants_subject_type_chk")
	addAt := strings.Index(flat, "add constraint resource_grants_subject_type_chk")
	if dropAt < 0 || addAt < 0 || dropAt > addAt {
		t.Fatal("resource_grants_subject_type_chk must be dropped (if exists) before it is re-added")
	}
	if !strings.Contains(flat, "check (subject_type in ('user', 'team', 'space'))") {
		t.Fatal("the subject_type CHECK must admit space next to user and team")
	}

	roleDrop := strings.Index(flat, "drop constraint if exists resource_grants_space_role_chk")
	roleAdd := strings.Index(flat, "add constraint resource_grants_space_role_chk")
	if roleDrop < 0 || roleAdd < 0 || roleDrop > roleAdd {
		t.Fatal("resource_grants_space_role_chk must be dropped (if exists) before it is re-added")
	}
	if !strings.Contains(flat, "check (subject_type <> 'space' or role = 'view')") {
		t.Fatal("a Space row must be view-only: CHECK (subject_type <> 'space' OR role = 'view')")
	}
	if strings.Contains(flat, "'edit'") {
		t.Fatal("043 must not mention the edit role: a Space row is never an editor")
	}
	if !strings.Contains(flat, "begin;") || !strings.Contains(flat, "commit;") {
		t.Fatal("migration 043 must run in one transaction")
	}
	if strings.Contains(flat, "create index") {
		t.Fatal("043 adds no index: idx_resource_grants_visible already leads with subject_id")
	}
}

func TestSpaceSubjectGrantMigrationDownDeletesSpaceRowsFirst(t *testing.T) {
	sql := readMigrationLower(t, "043_resource_grants_space_subject.down.sql")
	flat := strings.Join(strings.Fields(sql), " ")

	deleteAt := strings.Index(flat, "delete from resource_grants where subject_type = 'space'")
	narrowAt := strings.Index(flat, "check (subject_type in ('user', 'team'))")
	if deleteAt < 0 || narrowAt < 0 || deleteAt > narrowAt {
		t.Fatal("the down migration must delete Space rows before narrowing the subject_type CHECK")
	}
	if !strings.Contains(flat, "drop constraint if exists resource_grants_space_role_chk") {
		t.Fatal("the down migration must drop the Space role CHECK")
	}
	roleDrop := strings.Index(flat, "drop constraint if exists resource_grants_space_role_chk")
	if roleDrop < deleteAt {
		t.Fatal("the Space role CHECK is dropped after the Space rows are gone, not before")
	}
}
