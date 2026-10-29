package db

import (
	"testing"
)

func TestLoadEmbeddedMigrationsIsOrderedAndStartsAtBaseline(t *testing.T) {
	t.Parallel()

	entries, err := loadEmbeddedMigrations()
	if err != nil {
		t.Fatalf("loadEmbeddedMigrations: %v", err)
	}
	if len(entries) == 0 {
		t.Fatal("expected at least the baseline migration")
	}
	for i := 1; i < len(entries); i++ {
		if entries[i-1].version >= entries[i].version {
			t.Fatalf("migrations not strictly ascending: %d then %d", entries[i-1].version, entries[i].version)
		}
	}
	if entries[0].version != 1 || entries[0].body == "" {
		t.Fatalf("first migration = %+v, want version 1 with a body", entries[0])
	}
}

// The go:embed pattern must pick up the raw-store migration by its file name.
func TestEmbeddedMigrationsIncludeRawStore(t *testing.T) {
	t.Parallel()

	entries, err := loadEmbeddedMigrations()
	if err != nil {
		t.Fatalf("loadEmbeddedMigrations: %v", err)
	}
	for _, m := range entries {
		if m.version == 2 {
			if m.name != "0002_raw_store" || m.body == "" {
				t.Fatalf("version 2 = %q (body %d bytes), want 0002_raw_store with a body", m.name, len(m.body))
			}
			return
		}
	}
	t.Fatal("the embedded migrations have no version 2 (0002_raw_store.sql)")
}

// The embedded migrations are exactly versions 1 to 7, under these names: the
// baseline, the raw store, the run provenance guard, the terminal rejection count
// (0004, R5), the evidence run guard (0005, R6), the redaction mechanism (0006, O1) and
// the tenant schema for uploaded tender documents (0007).
// Migrations are forward-only, so a later change goes in a new file and never edits one
// of these.
func TestEmbeddedMigrationsAreVersionsOneToSeven(t *testing.T) {
	t.Parallel()

	entries, err := loadEmbeddedMigrations()
	if err != nil {
		t.Fatalf("loadEmbeddedMigrations: %v", err)
	}
	want := []string{
		"0001_baseline", "0002_raw_store", "0003_sync_guards",
		"0004_rejected_terminal", "0005_evidence_run_guard", "0006_redaction", "0007_tenant_documents",
		"0008_ted_source", "0009_workspace",
		"0010_workspace_seen", "0011_boamp_source", "0012_fts_source", "0013_bzp_source", "0014_catalogue_projection", "0015_catalogue_source_generation", "0016_catalogue_source_guard", "0017_matching_projection",
	}
	if len(entries) != len(want) {
		t.Fatalf("embedded migrations = %d, want %d", len(entries), len(want))
	}
	for i, m := range entries {
		if m.version != i+1 || m.name != want[i] || m.body == "" {
			t.Errorf("migration %d = version %d %q (body %d bytes), want version %d %q with a body",
				i+1, m.version, m.name, len(m.body), i+1, want[i])
		}
	}
}

func TestParseMigrationName(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name        string
		wantVersion int
		wantOK      bool
	}{
		{"0001_baseline.sql", 1, true},
		{"0042_add_raw_documents.sql", 42, true},
		{"1_short.sql", 0, false},
		{"0001.sql", 0, false},
		{"abcd_x.sql", 0, false},
		{"0001_x.txt", 0, false},
		{".gitkeep", 0, false},
		{"", 0, false},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			version, ok := parseMigrationName(tc.name)
			if ok != tc.wantOK || version != tc.wantVersion {
				t.Fatalf("parseMigrationName(%q) = %d, %v; want %d, %v", tc.name, version, ok, tc.wantVersion, tc.wantOK)
			}
		})
	}
}
