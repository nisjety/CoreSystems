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

func TestJetStreamInboxDeduplicatesRedelivery(t *testing.T) {
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
	const orgID = "org-jetstream-inbox-test"
	cleanup := func() {
		_, _ = pool.Exec(ctx, `DELETE FROM audit_events WHERE org_id = $1`, orgID)
		_, _ = pool.Exec(ctx, `DELETE FROM usage_events WHERE org_id = $1`, orgID)
	}
	cleanup()
	t.Cleanup(cleanup)

	audit := &events.AuditEvent{OccurredAt: time.Now(), OrgID: orgID, Plane: "control", Event: "dedupe"}
	inserted, err := st.InsertAuditFromStream(ctx, audit, "primary", 42)
	if err != nil || !inserted {
		t.Fatalf("first audit insert: inserted=%v err=%v", inserted, err)
	}
	inserted, err = st.InsertAuditFromStream(ctx, audit, "primary", 42)
	if err != nil || inserted {
		t.Fatalf("duplicate audit insert: inserted=%v err=%v", inserted, err)
	}

	usage := &events.UsageEvent{OccurredAt: time.Now(), OrgID: orgID, Plane: "model", Op: "tokens", CostCents: 1.25}
	inserted, err = st.InsertUsageFromStream(ctx, usage, "primary", 43)
	if err != nil || !inserted {
		t.Fatalf("first usage insert: inserted=%v err=%v", inserted, err)
	}
	inserted, err = st.InsertUsageFromStream(ctx, usage, "primary", 43)
	if err != nil || inserted {
		t.Fatalf("duplicate usage insert: inserted=%v err=%v", inserted, err)
	}

	var auditCount, usageCount int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM audit_events WHERE org_id = $1`, orgID).Scan(&auditCount); err != nil {
		t.Fatalf("count audit: %v", err)
	}
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM usage_events WHERE org_id = $1`, orgID).Scan(&usageCount); err != nil {
		t.Fatalf("count usage: %v", err)
	}
	if auditCount != 1 || usageCount != 1 {
		t.Fatalf("dedupe counts audit=%d usage=%d; want 1/1", auditCount, usageCount)
	}

	var migrationApplied bool
	if err := pool.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM schema_migrations WHERE version = '002_jetstream_inbox')`).Scan(&migrationApplied); err != nil {
		t.Fatalf("check migration ledger: %v", err)
	}
	if !migrationApplied {
		t.Fatal("002_jetstream_inbox migration was not recorded")
	}
}

func TestSummariseUsage_AggregatesAndOrdersByTotalCost(t *testing.T) {
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

	orgID := "org-usage-summary-" + time.Now().UTC().Format("20060102150405.000000000")
	cleanup := func() {
		_, _ = pool.Exec(ctx, `DELETE FROM usage_events WHERE org_id = $1`, orgID)
	}
	t.Cleanup(cleanup)
	st := store.New(pool)

	fixtures := []events.UsageEvent{
		{OccurredAt: time.Now(), OrgID: orgID, Plane: "model", Op: "inference", TokensIn: 10, CostCents: 3},
		{OccurredAt: time.Now(), OrgID: orgID, Plane: "model", Op: "inference", TokensIn: 20, CostCents: 2},
		{OccurredAt: time.Now(), OrgID: orgID, Plane: "application", Op: "export", BytesOut: 100, CostCents: 1},
	}
	for index := range fixtures {
		if _, err := st.InsertUsage(ctx, &fixtures[index]); err != nil {
			t.Fatalf("insert fixture %d: %v", index, err)
		}
	}

	rows, err := st.SummariseUsage(
		ctx,
		orgID,
		time.Now().Add(-time.Hour),
		time.Now().Add(time.Hour),
	)
	if err != nil {
		t.Fatalf("SummariseUsage: %v", err)
	}
	if len(rows) != 2 {
		t.Fatalf("summary row count = %d; want 2", len(rows))
	}
	if rows[0].Plane != "model" || rows[0].Op != "inference" {
		t.Fatalf("first row = %s/%s; want model/inference", rows[0].Plane, rows[0].Op)
	}
	if rows[0].Events != 2 || rows[0].TokensIn != 30 || rows[0].CostCents != 5 {
		t.Fatalf("model aggregate = %+v; want events=2 tokens_in=30 cost_cents=5", rows[0])
	}
}
