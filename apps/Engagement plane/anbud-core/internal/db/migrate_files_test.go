package db

import (
	"strings"
	"testing"
	"testing/fstest"
)

func TestLoadMigrationsOrdersByVersionAndIgnoresOtherFiles(t *testing.T) {
	t.Parallel()

	fsys := fstest.MapFS{
		"migrations/0002_second.sql": {Data: []byte("SELECT 2;")},
		"migrations/0001_first.sql":  {Data: []byte("SELECT 1;")},
		"migrations/README.md":       {Data: []byte("notes")},
		"migrations/.gitkeep":        {Data: nil},
	}
	got, err := loadMigrations(fsys, "migrations")
	if err != nil {
		t.Fatalf("loadMigrations: %v", err)
	}
	if len(got) != 2 || got[0].version != 1 || got[1].version != 2 {
		t.Fatalf("migrations = %+v, want versions 1 then 2", got)
	}
	if got[0].name != "0001_first" || got[0].body != "SELECT 1;" {
		t.Errorf("first migration = %+v", got[0])
	}
}

// A mistyped name used to be skipped silently: startup logged "migrations
// applied" and the table simply was not there.
func TestLoadMigrationsRefusesAnSQLFileItWouldSilentlySkip(t *testing.T) {
	t.Parallel()

	fsys := fstest.MapFS{
		"migrations/0001_baseline.sql": {Data: []byte("SELECT 1;")},
		"migrations/002_typo.sql":      {Data: []byte("SELECT 2;")},
	}
	_, err := loadMigrations(fsys, "migrations")
	if err == nil {
		t.Fatal("expected an error for a .sql file that does not match NNNN_description.sql")
	}
	if !strings.Contains(err.Error(), "002_typo.sql") {
		t.Fatalf("the error should name the offending file: %v", err)
	}
}

func TestLoadMigrationsRefusesDuplicateVersions(t *testing.T) {
	t.Parallel()

	fsys := fstest.MapFS{
		"migrations/0001_a.sql": {Data: []byte("SELECT 1;")},
		"migrations/0001_b.sql": {Data: []byte("SELECT 2;")},
	}
	if _, err := loadMigrations(fsys, "migrations"); err == nil {
		t.Fatal("expected an error for two migrations with the same version")
	}
}

// The suffix check was case-sensitive, so 0002_raw.SQL fell through the same
// silent skip as a mistyped name. It must fail the load instead.
func TestLoadMigrationsRejectsUpperCaseSQLSuffix(t *testing.T) {
	t.Parallel()

	fsys := fstest.MapFS{
		"migrations/0001_baseline.sql": {Data: []byte("SELECT 1;")},
		"migrations/0002_raw.SQL":      {Data: []byte("SELECT 2;")},
	}
	_, err := loadMigrations(fsys, "migrations")
	if err == nil {
		t.Fatal("expected an error for a migration whose suffix is not lower-case .sql")
	}
	if !strings.Contains(err.Error(), "0002_raw.SQL") {
		t.Fatalf("the error should name the offending file: %v", err)
	}
}
