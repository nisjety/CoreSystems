package health

import (
	"context"
	"testing"
	"time"
)

// TestCheckPostgreSQLWithoutDSNIsNotConfigured covers a boot with no
// DATABASE_POSTGRESURL. An empty DSN must return before sql.Open, because lib/pq
// would then dial localhost:5432 or whatever the PG* environment names. The PG*
// variables here point at a closed port, so an accidental dial fails rather than
// quietly succeeding against a database on the test machine.
func TestCheckPostgreSQLWithoutDSNIsNotConfigured(t *testing.T) {
	t.Setenv("PGHOST", "127.0.0.1")
	t.Setenv("PGPORT", "1")

	checker := NewDatabaseChecker(Config{PostgresURL: ""})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	start := time.Now()
	got := checker.CheckPostgreSQL(ctx)
	elapsed := time.Since(start)

	if got.Status != "not_configured" {
		t.Fatalf("Status = %q, want %q", got.Status, "not_configured")
	}
	if got.Error != "" {
		t.Fatalf("Error = %q, want empty for an unconfigured database", got.Error)
	}
	if elapsed > time.Second {
		t.Fatalf("CheckPostgreSQL took %v without a DSN; it must not open a connection", elapsed)
	}
}
