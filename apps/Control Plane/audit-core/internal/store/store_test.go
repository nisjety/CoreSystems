package store_test

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// TestPurge_RejectsNonPositiveRetention verifies the guard fires before any
// SQL runs. We pass a nil-pool store: if the guard did not short-circuit, the
// nil pool would panic, so reaching a clean error return proves the guard
// protects against a misconfiguration purging the entire table.
func TestPurge_RejectsNonPositiveRetention(t *testing.T) {
	st := store.New(nil)

	for _, days := range []int{0, -1, -365} {
		if _, err := st.Purge(context.Background(), days); err == nil {
			t.Fatalf("Purge(%d) = nil error; want rejection", days)
		}
	}
}

// TestPurge_DeletesOnlyExpiredRows is an integration test gated on
// TEST_DATABASE_URL. It seeds rows on both sides of the retention cutoff and
// asserts the purge removes only the expired rows and reports an accurate
// count, leaving the recent rows intact.
func TestPurge_DeletesOnlyExpiredRows(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping Postgres integration test")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()

	if err := store.Migrate(ctx, pool); err != nil {
		t.Fatalf("migrate: %v", err)
	}

	st := store.New(pool)
	const orgID = "org-purge-test"

	// Clean slate for this org so reruns are deterministic.
	cleanup := func() {
		_, _ = pool.Exec(ctx, `DELETE FROM audit_events WHERE org_id = $1`, orgID)
		_, _ = pool.Exec(ctx, `DELETE FROM usage_events WHERE org_id = $1`, orgID)
	}
	cleanup()
	t.Cleanup(cleanup)

	// Insert one recent and one expired row into each table. ingested_at
	// defaults to NOW() on insert, so we backdate the expired rows directly.
	if _, err := st.InsertAudit(ctx, &events.AuditEvent{
		OccurredAt: time.Now(), OrgID: orgID, Plane: "control", Event: "recent",
	}); err != nil {
		t.Fatalf("insert recent audit: %v", err)
	}
	if _, err := st.InsertAudit(ctx, &events.AuditEvent{
		OccurredAt: time.Now(), OrgID: orgID, Plane: "control", Event: "expired",
	}); err != nil {
		t.Fatalf("insert expired audit: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		UPDATE audit_events SET ingested_at = NOW() - INTERVAL '400 days'
		WHERE org_id = $1 AND event = 'expired'`, orgID); err != nil {
		t.Fatalf("backdate audit: %v", err)
	}

	if _, err := st.InsertUsage(ctx, &events.UsageEvent{
		OccurredAt: time.Now(), OrgID: orgID, Plane: "control", Op: "recent",
	}); err != nil {
		t.Fatalf("insert recent usage: %v", err)
	}
	if _, err := st.InsertUsage(ctx, &events.UsageEvent{
		OccurredAt: time.Now(), OrgID: orgID, Plane: "control", Op: "expired",
	}); err != nil {
		t.Fatalf("insert expired usage: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		UPDATE usage_events SET ingested_at = NOW() - INTERVAL '400 days'
		WHERE org_id = $1 AND op = 'expired'`, orgID); err != nil {
		t.Fatalf("backdate usage: %v", err)
	}

	res, err := st.Purge(ctx, 365)
	if err != nil {
		t.Fatalf("Purge: %v", err)
	}
	if res.AuditDeleted != 1 {
		t.Errorf("AuditDeleted = %d; want 1", res.AuditDeleted)
	}
	if res.UsageDeleted != 1 {
		t.Errorf("UsageDeleted = %d; want 1", res.UsageDeleted)
	}

	// The recent rows must survive.
	var auditRemaining, usageRemaining int
	if err := pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM audit_events WHERE org_id = $1`, orgID).Scan(&auditRemaining); err != nil {
		t.Fatalf("count audit: %v", err)
	}
	if err := pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM usage_events WHERE org_id = $1`, orgID).Scan(&usageRemaining); err != nil {
		t.Fatalf("count usage: %v", err)
	}
	if auditRemaining != 1 {
		t.Errorf("audit rows remaining = %d; want 1 (recent only)", auditRemaining)
	}
	if usageRemaining != 1 {
		t.Errorf("usage rows remaining = %d; want 1 (recent only)", usageRemaining)
	}
}
