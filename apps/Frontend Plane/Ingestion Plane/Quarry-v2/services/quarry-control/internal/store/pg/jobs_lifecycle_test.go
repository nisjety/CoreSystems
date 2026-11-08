package pg

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// TestJobsMigration016IsIdempotent checks that every DDL statement of migration
// 016 is guarded, so a second application is a no-op, and, when a database is
// configured (QUARRY_TEST_DSN), applies the file twice to prove it.
func TestJobsMigration016IsIdempotent(t *testing.T) {
	raw, err := migrationFS.ReadFile("migrations/016_jobs_lifecycle.sql")
	if err != nil {
		t.Fatal(err)
	}
	statements := 0
	for _, line := range strings.Split(string(raw), "\n") {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "ALTER TABLE") || strings.HasPrefix(trimmed, "CREATE INDEX") {
			statements++
			if !strings.Contains(trimmed, "IF NOT EXISTS") {
				t.Fatalf("unguarded DDL: %s", trimmed)
			}
		}
	}
	if statements == 0 {
		t.Fatal("migration 016 has no DDL statements")
	}

	dsn := testDSN(t)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("pgxpool.New: %v", err)
	}
	defer pool.Close()
	for run := 1; run <= 2; run++ {
		if _, err := pool.Exec(ctx, string(raw)); err != nil {
			t.Fatalf("migration 016 run %d: %v", run, err)
		}
	}
}
