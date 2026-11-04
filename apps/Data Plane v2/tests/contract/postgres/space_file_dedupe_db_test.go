package postgrescontract

import (
	"strings"
	"testing"
)

// WS14-S3 against a real Postgres: the migration applies and reverses, and the
// index is the backstop for one live room file per content hash per Space.

func sfdRoomFile(id, org, space, sha, deleted string) string {
	return "INSERT INTO documents (document_id, org_id, source, type, title, content, metadata, space_ref, deleted_at) VALUES (" +
		q(id) + ", " + q(org) + ", 'space-upload', 'space_upload', 't', 'c', " +
		q(`{"sha256":"`+sha+`"}`) + "::jsonb, " + space + ", " + deleted + ")"
}

func TestSpaceFileDedupeMigrationUpDownAndIndexLifecycle(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)

	migrate(t, db, "up", "--to", gesVersion)
	const exists = `SELECT count(*) FROM pg_indexes WHERE schemaname = 'public' AND indexname = 'uq_documents_space_file_sha256'`
	if queryInt(t, pool, exists) != 0 {
		t.Fatal("the room-file index exists before its migration")
	}

	migrate(t, db, "up", "--to", sfdVersion)
	if queryInt(t, pool, exists) != 1 {
		t.Fatal("uq_documents_space_file_sha256 is missing after up")
	}
	def := queryString(t, pool, `SELECT indexdef FROM pg_indexes WHERE indexname = 'uq_documents_space_file_sha256'`)
	for _, want := range []string{"UNIQUE", "org_id", "space_ref", "sha256", "space_upload"} {
		if !strings.Contains(def, want) {
			t.Errorf("index definition %q lacks %q", def, want)
		}
	}

	// Rows survive a rollback: only the index goes.
	execSQL(t, pool, sfdRoomFile("sfd-1", "org-sfd", q("space:org-sfd:r1"), sha256hex("a"), "NULL"))
	migrate(t, db, "down", "--to", gesVersion)
	if queryInt(t, pool, exists) != 0 {
		t.Fatal("the index survives the down migration")
	}
	if queryInt(t, pool, `SELECT count(*) FROM documents WHERE document_id = 'sfd-1'`) != 1 {
		t.Fatal("the down migration deleted a room-file row")
	}
	migrate(t, db, "up", "--to", sfdVersion)
	if queryInt(t, pool, exists) != 1 {
		t.Fatal("up after down did not restore the index")
	}
}

func TestSpaceFileDedupeIndexScopeAfterMigration(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)
	migrate(t, db, "up", "--to", sfdVersion)
	h := sha256hex("same bytes")

	execSQL(t, pool, sfdRoomFile("a1", "org-a", q("space:org-a:r1"), h, "NULL"))
	// Same org + Space + hash while the first is live: the index refuses it.
	_, err := pool.Exec(t.Context(), sfdRoomFile("a2", "org-a", q("space:org-a:r1"), h, "NULL"))
	expectPgError(t, err, "23505", "uq_documents_space_file_sha256")

	// Different Space, different org: allowed.
	execSQL(t, pool, sfdRoomFile("a3", "org-a", q("space:org-a:r2"), h, "NULL"))
	execSQL(t, pool, sfdRoomFile("b1", "org-b", q("space:org-a:r1"), h, "NULL"))
	// A deleted room file does not hold the hash, and does not collide with itself.
	execSQL(t, pool, sfdRoomFile("a4", "org-a", q("space:org-a:r3"), h, "now()"))
	execSQL(t, pool, sfdRoomFile("a5", "org-a", q("space:org-a:r3"), h, "NULL"))
	// Org-wide documents (no Space) and other types are outside the index.
	execSQL(t, pool, sfdRoomFile("n1", "org-a", "NULL", h, "NULL"))
	execSQL(t, pool, sfdRoomFile("n2", "org-a", "NULL", h, "NULL"))
	execSQL(t, pool, `INSERT INTO documents (document_id, org_id, source, type, title, content, metadata, space_ref)
		VALUES ('o1', 'org-a', 's', 'notion', 't', 'c', '{"sha256":"`+h+`"}'::jsonb, 'space:org-a:r1'),
		       ('o2', 'org-a', 's', 'notion', 't', 'c', '{"sha256":"`+h+`"}'::jsonb, 'space:org-a:r1')`)
}
