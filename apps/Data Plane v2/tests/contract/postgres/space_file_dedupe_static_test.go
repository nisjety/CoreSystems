package postgrescontract

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// WS14-S3: the room-file dedupe migration. File-reading only, so it runs
// everywhere, always. The database behaviour is in space_file_dedupe_db_test.go.

const (
	sfdVersion = "20261102062000"
	sfdUp      = "20261102062000_documents_space_file_dedupe.sql"
	sfdDown    = "20261102062000_documents_space_file_dedupe.down.sql"
	sfdIndex   = "uq_documents_space_file_sha256"
)

func TestSpaceFileDedupeMigrationIsAPairWithAFreeVersionAfterGraphExtractionState(t *testing.T) {
	entries, err := os.ReadDir(migrationsDir(t))
	if err != nil {
		t.Fatal(err)
	}
	forward := regexp.MustCompile(`^(\d{14})_(.+)\.sql$`)
	var claimed []string
	for _, e := range entries {
		name := e.Name()
		if e.IsDir() || strings.HasSuffix(name, ".disabled") || strings.HasSuffix(name, ".down.sql") {
			continue
		}
		if m := forward.FindStringSubmatch(name); m != nil && m[1] == sfdVersion {
			claimed = append(claimed, name)
		}
	}
	if len(claimed) != 1 || claimed[0] != sfdUp {
		t.Errorf("forward migrations claiming %s = %v, want exactly [%s]", sfdVersion, claimed, sfdUp)
	}
	if sfdVersion <= gesVersion {
		t.Errorf("%s must sort after %s", sfdVersion, gesVersion)
	}
	if _, err := os.Stat(filepath.Join(migrationsDir(t), sfdDown)); err != nil {
		t.Errorf("a migration without a paired .down.sql cannot be rolled back: %v", err)
	}
}

func TestSpaceFileDedupeUpIsIdempotentAndScopedToLiveRoomFiles(t *testing.T) {
	sql := code(readSQL(t, sfdUp))
	if tx := regexp.MustCompile(`(?im)^\s*(BEGIN|COMMIT|ROLLBACK|START\s+TRANSACTION)\s*;`).FindString(sql); tx != "" {
		t.Errorf("transaction control %q: the migrator owns the transaction", strings.TrimSpace(tx))
	}
	if strings.Contains(strings.ToUpper(sql), "CONCURRENTLY") {
		t.Error("CREATE INDEX CONCURRENTLY cannot run inside the migrator's transaction")
	}
	create := regexp.MustCompile(`(?is)CREATE\s+UNIQUE\s+INDEX\s+IF\s+NOT\s+EXISTS\s+` + sfdIndex + `\s+ON\s+documents\s*\((.*?)\)\s*WHERE\s+(.*?);`)
	m := create.FindStringSubmatch(sql)
	if m == nil {
		t.Fatalf("the migration does not CREATE UNIQUE INDEX IF NOT EXISTS %s ON documents ... WHERE ...", sfdIndex)
	}
	columns := strings.Join(strings.Fields(strings.ToLower(m[1])), " ")
	if columns != "org_id, space_ref, (metadata->>'sha256')" {
		t.Errorf("index columns = %q, want (org_id, space_ref, (metadata->>'sha256')): one hash per org and Space", columns)
	}
	predicate := strings.Join(strings.Fields(strings.ToLower(m[2])), " ")
	for _, want := range []string{"type = 'space_upload'", "deleted_at is null", "space_ref is not null"} {
		if !strings.Contains(predicate, want) {
			t.Errorf("index predicate %q lacks %q: it would cover other document types, deleted files or org-wide documents", predicate, want)
		}
	}
	if n := len(regexp.MustCompile(`(?i)\b(ALTER|DROP|DELETE|UPDATE|INSERT)\b`).FindAllString(sql, -1)); n != 0 {
		t.Errorf("the migration is an index only; found %d data or table statements", n)
	}
}

func TestSpaceFileDedupeDownDropsOnlyTheIndexAndTheRepoNamesTheSameIndex(t *testing.T) {
	down := code(readSQL(t, sfdDown))
	if got := strings.Join(strings.Fields(strings.TrimSpace(down)), " "); !strings.EqualFold(got, "DROP INDEX IF EXISTS "+sfdIndex+";") {
		t.Errorf("down migration = %q, want only DROP INDEX IF EXISTS %s;", got, sfdIndex)
	}
	// documents-api maps a unique violation to room_file_duplicate by this name.
	raw, err := os.ReadFile(filepath.Join(dataPlaneRoot(t), "services", "documents-api-go", "internal", "repo", "space_files.go"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(raw), `"`+sfdIndex+`"`) {
		t.Errorf("space_files.go no longer names %s: the unique-index backstop would not map to a duplicate", sfdIndex)
	}
}
