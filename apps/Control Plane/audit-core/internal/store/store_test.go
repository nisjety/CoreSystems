package store_test

import (
	"context"
	"errors"
	"fmt"
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

func TestDirectUsageLogicalIdentityDeduplicatesExactRetryAndRejectsConflict(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping Postgres integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	if err := store.Migrate(ctx, pool); err != nil {
		t.Fatal(err)
	}

	now := time.Now().UTC()
	orgID := fmt.Sprintf("org-direct-usage-%d", now.UnixNano())
	t.Cleanup(func() { _, _ = pool.Exec(context.Background(), `DELETE FROM usage_events WHERE org_id = $1`, orgID) })
	event := &events.UsageEvent{
		EventID: "usage:integration:request-1", OccurredAt: now, OrgID: orgID,
		Plane: "ingestion", Producer: "integration-corev2", Op: "documents", BytesIn: 42, CostCents: 1.25,
	}
	st := store.New(pool)
	inserted, err := st.InsertUsageFromSource(ctx, event, "http:integration-corev2")
	if err != nil || !inserted {
		t.Fatalf("first direct usage insert: inserted=%v err=%v", inserted, err)
	}
	inserted, err = st.InsertUsageFromSource(ctx, event, "http:integration-corev2")
	if err != nil || inserted {
		t.Fatalf("exact direct usage retry: inserted=%v err=%v", inserted, err)
	}
	conflict := *event
	conflict.CostCents = 99
	if _, err := st.InsertUsageFromSource(ctx, &conflict, "http:integration-corev2"); !errors.Is(err, store.ErrUsageEventConflict) {
		t.Fatalf("conflicting direct usage retry error = %v", err)
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
		EventID: "audit-retention-recent", OccurredAt: time.Now(), OrgID: orgID,
		Plane: "control", Producer: "audit-core-test", Event: "recent",
	}); err != nil {
		t.Fatalf("insert recent audit: %v", err)
	}
	if _, err := st.InsertAudit(ctx, &events.AuditEvent{
		EventID: "audit-retention-expired", OccurredAt: time.Now(), OrgID: orgID,
		Plane: "control", Producer: "audit-core-test", Event: "expired",
	}); err != nil {
		t.Fatalf("insert expired audit: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		UPDATE audit_events SET ingested_at = NOW() - INTERVAL '400 days'
		WHERE org_id = $1 AND event = 'expired'`, orgID); err != nil {
		t.Fatalf("backdate audit: %v", err)
	}

	if _, err := st.InsertUsage(ctx, &events.UsageEvent{
		EventID: "usage-retention-recent", OccurredAt: time.Now(), OrgID: orgID,
		Plane: "control", Producer: "audit-core-test", Op: "recent",
	}); err != nil {
		t.Fatalf("insert recent usage: %v", err)
	}
	if _, err := st.InsertUsage(ctx, &events.UsageEvent{
		EventID: "usage-retention-expired", OccurredAt: time.Now(), OrgID: orgID,
		Plane: "control", Producer: "audit-core-test", Op: "expired",
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

	audit := &events.AuditEvent{
		EventID: "audit-auth-dedupe", OccurredAt: time.Now(), OrgID: orgID,
		Plane: "control", Producer: "auth-core", Event: "dedupe",
	}
	inserted, err := st.InsertAuditFromStream(ctx, audit, "primary", "velion.audit.v2.control.auth-core.dedupe", 42)
	if err != nil || !inserted {
		t.Fatalf("first audit insert: inserted=%v err=%v", inserted, err)
	}
	inserted, err = st.InsertAuditFromStream(ctx, audit, "primary", "velion.audit.v2.control.auth-core.dedupe", 42)
	if err != nil || inserted {
		t.Fatalf("duplicate audit insert: inserted=%v err=%v", inserted, err)
	}

	usage := &events.UsageEvent{
		EventID: "usage-stream-sequence", OccurredAt: time.Now(), OrgID: orgID,
		Plane: "model", Producer: "session-core", Op: "tokens", CostCents: 1.25,
	}
	inserted, err = st.InsertUsageFromStream(ctx, usage, "primary", "velion.usage.v2.model.session-core.tokens", 43)
	if err != nil || !inserted {
		t.Fatalf("first usage insert: inserted=%v err=%v", inserted, err)
	}
	inserted, err = st.InsertUsageFromStream(ctx, usage, "primary", "velion.usage.v2.model.session-core.tokens", 43)
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

func TestLogicalAuditIdentityIsBoundToProducerAndRejectsConflictingReuse(t *testing.T) {
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
	now := time.Now().UnixNano()
	orgID := fmt.Sprintf("org-logical-audit-%d", now)
	eventID := fmt.Sprintf("membership:%s:user-1:1:member_added", orgID)
	sourceBus := fmt.Sprintf("logical-audit-%d", now)
	firstSequence := uint64(now)
	cleanup := func() {
		_, _ = pool.Exec(ctx, `DELETE FROM audit_events WHERE org_id = $1`, orgID)
	}
	cleanup()
	t.Cleanup(cleanup)

	audit := &events.AuditEvent{
		EventID:    eventID,
		OccurredAt: time.Now(),
		OrgID:      orgID,
		Plane:      "control",
		Producer:   "auth-core",
		Event:      "member_added",
	}
	authSubject := "velion.audit.v2.control.auth-core.member_added"
	userSubject := "velion.audit.v2.control.user-core.member_added"
	inserted, err := st.InsertAuditFromStream(ctx, audit, sourceBus, authSubject, firstSequence)
	if err != nil || !inserted {
		t.Fatalf("first logical audit insert: inserted=%v err=%v", inserted, err)
	}
	inserted, err = st.InsertAuditFromStream(ctx, audit, sourceBus, authSubject, firstSequence+1)
	if err != nil || inserted {
		t.Fatalf("logical duplicate with new stream sequence: inserted=%v err=%v", inserted, err)
	}
	conflict := *audit
	conflict.Outcome = "denied"
	if _, err := st.InsertAuditFromStream(ctx, &conflict, sourceBus, authSubject, firstSequence+2); !errors.Is(err, store.ErrAuditEventConflict) {
		t.Fatalf("conflicting logical audit reuse error = %v", err)
	}
	inserted, err = st.InsertAuditFromStream(ctx, audit, sourceBus, "velion.audit.v2.control.auth-core.role_changed", firstSequence+3)
	if err != nil || inserted {
		t.Fatalf("same producer/event id on a different suffix: inserted=%v err=%v", inserted, err)
	}
	otherProducer := *audit
	otherProducer.Producer = "user-core"
	inserted, err = st.InsertAuditFromStream(ctx, &otherProducer, sourceBus, userSubject, firstSequence+4)
	if err != nil || !inserted {
		t.Fatalf("same event id from a separate scoped producer: inserted=%v err=%v", inserted, err)
	}

	var count int
	if err := pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM audit_events WHERE source_bus = $1 AND event_id = $2`,
		sourceBus, eventID,
	).Scan(&count); err != nil {
		t.Fatalf("count logical audit rows: %v", err)
	}
	if count != 2 {
		t.Fatalf("logical audit rows = %d; want 2 isolated by producer", count)
	}
}

func TestLogicalUsageIdempotencySurvivesNewStreamSequence(t *testing.T) {
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
	now := time.Now().UnixNano()
	orgID := fmt.Sprintf("org-logical-usage-%d", now)
	eventID := fmt.Sprintf("usage:model:%d", now)
	sourceBus := fmt.Sprintf("logical-usage-%d", now)
	sourceSubject := "velion.usage.v2.model.session-core.tokens"
	firstSequence := uint64(now)
	cleanup := func() { _, _ = pool.Exec(ctx, `DELETE FROM usage_events WHERE org_id = $1`, orgID) }
	cleanup()
	t.Cleanup(cleanup)

	usage := &events.UsageEvent{
		OccurredAt: time.Now(), OrgID: orgID, Plane: "model", Producer: "session-core", Op: "tokens",
		EventID: eventID, CostCents: 1.25,
	}
	inserted, err := st.InsertUsageFromStream(ctx, usage, sourceBus, sourceSubject, firstSequence)
	if err != nil || !inserted {
		t.Fatalf("first logical usage insert: inserted=%v err=%v", inserted, err)
	}
	inserted, err = st.InsertUsageFromStream(ctx, usage, sourceBus, sourceSubject, firstSequence+1)
	if err != nil || inserted {
		t.Fatalf("logical usage duplicate with new stream sequence: inserted=%v err=%v", inserted, err)
	}

	var count int
	if err := pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM usage_events WHERE source_bus = $1 AND source_subject = $2 AND event_id = $3`,
		sourceBus, sourceSubject, eventID,
	).Scan(&count); err != nil {
		t.Fatalf("count logical usage rows: %v", err)
	}
	if count != 1 {
		t.Fatalf("logical usage rows = %d; want 1", count)
	}
	inserted, err = st.InsertUsageFromStream(ctx, usage, sourceBus, "velion.usage.v2.model.session-core.tokens-secondary", firstSequence+2)
	if err != nil || inserted {
		t.Fatalf("same producer/event id on a different suffix: inserted=%v err=%v", inserted, err)
	}
	otherProducer := *usage
	otherProducer.Producer = "model-gateway"
	inserted, err = st.InsertUsageFromStream(ctx, &otherProducer, sourceBus, "velion.usage.v2.model.model-gateway.tokens", firstSequence+3)
	if err != nil || !inserted {
		t.Fatalf("independent scoped producer was preempted: inserted=%v err=%v", inserted, err)
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
		{EventID: "usage-summary-1", OccurredAt: time.Now(), OrgID: orgID, Plane: "model", Producer: "session-core", Op: "inference", TokensIn: 10, CostCents: 3},
		{EventID: "usage-summary-2", OccurredAt: time.Now(), OrgID: orgID, Plane: "model", Producer: "session-core", Op: "inference", TokensIn: 20, CostCents: 2},
		{EventID: "usage-summary-3", OccurredAt: time.Now(), OrgID: orgID, Plane: "application", Producer: "leads-core", Op: "export", BytesOut: 100, CostCents: 1},
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
