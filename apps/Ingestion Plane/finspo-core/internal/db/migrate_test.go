package db

import "testing"

func TestLoadEmbeddedMigrationsIsOrderedAndNonEmpty(t *testing.T) {
	t.Parallel()

	entries, err := loadEmbeddedMigrations()
	if err != nil {
		t.Fatalf("loadEmbeddedMigrations: %v", err)
	}
	if len(entries) == 0 {
		t.Fatal("expected at least one embedded migration")
	}

	for i := 1; i < len(entries); i++ {
		if entries[i-1].version >= entries[i].version {
			t.Fatalf("migrations not strictly ascending: %d then %d", entries[i-1].version, entries[i].version)
		}
	}

	first := entries[0]
	if first.version != 1 {
		t.Fatalf("first migration version = %d, want 1", first.version)
	}
	if first.body == "" {
		t.Fatalf("first migration body is empty")
	}
}
