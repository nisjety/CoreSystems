package db

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestApplyMigrationsAuditOutboxOnDisposablePostgres(t *testing.T) {
	databaseURL := strings.TrimSpace(os.Getenv("INTEGRATION_TEST_DATABASE_URL"))
	if databaseURL == "" {
		t.Skip("INTEGRATION_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		t.Fatalf("open disposable postgres: %v", err)
	}
	defer pool.Close()

	var databaseName string
	if err := pool.QueryRow(ctx, `SELECT current_database()`).Scan(&databaseName); err != nil {
		t.Fatalf("read database name: %v", err)
	}
	if normalized := strings.ToLower(databaseName); !strings.Contains(normalized, "test") && !strings.Contains(normalized, "tmp") {
		t.Fatalf("refusing migration test against non-disposable database %q", databaseName)
	}

	if err := ApplyMigrations(ctx, pool); err != nil {
		t.Fatalf("apply migrations: %v", err)
	}
	if err := ApplyMigrations(ctx, pool); err != nil {
		t.Fatalf("reapply migrations: %v", err)
	}

	var applied bool
	if err := pool.QueryRow(ctx, `SELECT EXISTS (
		SELECT 1 FROM integration_schema_migrations WHERE version = $1
	)`, latestMigration).Scan(&applied); err != nil {
		t.Fatalf("query latest migration: %v", err)
	}
	if !applied {
		t.Fatalf("latest migration %q was not recorded", latestMigration)
	}

	eventID := "postgres-outbox-" + uuid.NewString()
	if _, err := pool.Exec(ctx, `INSERT INTO integration_audit_events (
		id, organization_id, event_type, request_id
	) VALUES ($1, 'org-postgres-test', 'connection.tested', 'request-postgres-test')`, eventID); err != nil {
		t.Fatalf("insert pending audit event: %v", err)
	}

	var claimedID string
	var attempts int
	if err := pool.QueryRow(ctx, `SELECT id, attempts FROM claim_integration_audit_event()`).Scan(&claimedID, &attempts); err != nil {
		t.Fatalf("claim pending audit event: %v", err)
	}
	if claimedID != eventID || attempts != 1 {
		t.Fatalf("claimed event = (%q, %d), want (%q, 1)", claimedID, attempts, eventID)
	}
	if _, err := pool.Exec(ctx, `UPDATE integration_audit_events
		SET terminal_at = now(), processing_at = NULL WHERE id = $1`, eventID); err != nil {
		t.Fatalf("mark disposable event terminal: %v", err)
	}
	repository := store.NewPostgresRepository(pool)
	stats, err := repository.AuditOutboxStats(ctx)
	if err != nil {
		t.Fatalf("query real audit outbox stats: %v", err)
	}
	if stats.Terminal != 1 || stats.OldestTerminalAge < 0 {
		t.Fatalf("real audit outbox stats = %#v, want one terminal event", stats)
	}
	requeuedTerminal, err := repository.RequeueTerminalAuditEvents(ctx, []string{eventID})
	if err != nil {
		t.Fatalf("requeue real terminal audit event: %v", err)
	}
	if requeuedTerminal != 1 {
		t.Fatalf("requeued terminal rows = %d, want 1", requeuedTerminal)
	}

	legacyID := "postgres-legacy-" + uuid.NewString()
	if _, err := pool.Exec(ctx, `INSERT INTO integration_audit_events (
		id, organization_id, event_type, legacy_pre_outbox, published_at
	) VALUES ($1, 'org-postgres-test', 'connection.legacy', TRUE, now())`, legacyID); err != nil {
		t.Fatalf("insert quarantined legacy audit event: %v", err)
	}

	var requeued int
	if err := pool.QueryRow(ctx, `SELECT requeue_legacy_integration_audit_events(ARRAY[$1]::TEXT[])`, legacyID).Scan(&requeued); err != nil {
		t.Fatalf("requeue bounded legacy event: %v", err)
	}
	if requeued != 1 {
		t.Fatalf("requeued rows = %d, want 1", requeued)
	}

	var publishedAt *time.Time
	if err := pool.QueryRow(ctx, `SELECT published_at FROM integration_audit_events WHERE id = $1`, legacyID).Scan(&publishedAt); err != nil {
		t.Fatalf("read requeued legacy event: %v", err)
	}
	if publishedAt != nil {
		t.Fatalf("requeued legacy event published_at = %v, want NULL", publishedAt)
	}
}
