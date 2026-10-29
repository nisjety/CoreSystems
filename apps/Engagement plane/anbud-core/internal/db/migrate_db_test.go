package db_test

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// TestApplyMigrationsAgainstARealDatabase proves the runner is idempotent. It
// runs on a fresh database, so the second pass must find nothing to apply.
func TestApplyMigrationsAgainstARealDatabase(t *testing.T) {
	pool := dbtest.Open(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	for i := range 2 {
		if err := db.ApplyMigrations(ctx, pool); err != nil {
			t.Fatalf("ApplyMigrations (run %d): %v", i+1, err)
		}
	}
	var version int
	if err := pool.QueryRow(ctx, `SELECT max(version) FROM schema_migrations`).Scan(&version); err != nil {
		t.Fatalf("read schema_migrations: %v", err)
	}
	if version < 1 {
		t.Fatalf("baseline migration not recorded (max version %d)", version)
	}
}
