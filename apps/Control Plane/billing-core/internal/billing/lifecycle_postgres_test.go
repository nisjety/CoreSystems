package billing

import (
	"context"
	"errors"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/testfixture"
	"github.com/jackc/pgx/v5/pgxpool"
)

type idempotentUsageFixtureAdapter struct {
	mu       sync.Mutex
	calls    []string
	accepted map[string]int
}

type usageFixturePublisher struct {
	eventIDs []string
}

func (p *usageFixturePublisher) Publish(_ context.Context, subject string, payload map[string]any) error {
	if subject == "billing.usage.recorded" {
		if eventID, ok := payload["event_id"].(string); ok {
			p.eventIDs = append(p.eventIDs, eventID)
		}
	}
	return nil
}

func (a *idempotentUsageFixtureAdapter) ReportUsage(_ context.Context, usage UsageEvent) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.accepted == nil {
		a.accepted = make(map[string]int)
	}
	a.calls = append(a.calls, usage.EventID)
	a.accepted[usage.EventID]++
	return nil
}

func (a *idempotentUsageFixtureAdapter) counts(eventID string) (int, int) {
	a.mu.Lock()
	defer a.mu.Unlock()
	unique := 0
	if a.accepted[eventID] > 0 {
		unique = 1
	}
	return len(a.calls), unique
}

type lifecycleSharedPublisher struct {
	planFailures int
	revisions    []int64
}

func (*lifecycleSharedPublisher) PublishAccountUpdated(context.Context, string, string, int) {}
func (*lifecycleSharedPublisher) PublishInvoiceCreated(context.Context, string, string, int64, string) {
}
func (*lifecycleSharedPublisher) PublishQuotaExceeded(context.Context, string, string, int64, int64) {
}
func (p *lifecycleSharedPublisher) PublishPlanChanged(_ context.Context, _ string, _ string, _ string, revision ...int64) error {
	if len(revision) > 0 {
		p.revisions = append(p.revisions, revision[0])
	}
	if p.planFailures > 0 {
		p.planFailures--
		return errors.New("fixture shared plan PubAck failure")
	}
	return nil
}
func (*lifecycleSharedPublisher) PublishPlain(string, map[string]any) {}

// TestControlLifecycleBillingTombstoneRejectsDelayedPlanEvents applies the
// actual billing migrations to an explicitly configured disposable database.
// It never calls Lago or a payment provider.
func TestControlLifecycleBillingTombstoneRejectsDelayedPlanEvents(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(
		ctx,
		db.Pool,
		dsn,
		"billing_lifecycle",
		os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply billing-core migrations: %v", err)
	}

	repo := NewRepository(db.Pool)
	service := NewService(repo, nil, nil)
	// This lifecycle fixture deliberately has no network dependencies: default
	// hydration falls back to the persisted plan when Org Core is unavailable.
	service.orgServiceURL = ""
	canceledCtx, cancelOperation := context.WithCancel(ctx)
	cancelOperation()
	if err := repo.TombstoneOrganization(canceledCtx, "billing-lifecycle-canceled", "canceled"); err == nil {
		t.Fatal("canceled tombstone operation unexpectedly succeeded")
	}

	orgID := "billing-lifecycle-tombstone"
	if err := service.UpsertAccount(ctx, Account{
		OrgID:             orgID,
		Plan:              "pro",
		SubscriptionState: SubscriptionStateActive,
		Entitlements:      map[string]bool{"feature.integrations": true},
		QuotaLimits:       map[string]float64{"api_calls": 10_000},
	}); err != nil {
		t.Fatalf("seed isolated billing account: %v", err)
	}

	// The canonical deletion path is retry-safe and atomically records the
	// permanent anti-resurrection tombstone with cancellation state.
	if err := service.DeactivateOrganization(ctx, orgID, " \t "); err != nil {
		t.Fatalf("deactivate organization billing: %v", err)
	}
	if err := service.DeactivateOrganization(ctx, orgID, "organization_deleted_retry"); err != nil {
		t.Fatalf("retry organization billing deactivation: %v", err)
	}

	account, err := repo.GetAccount(ctx, orgID)
	if err != nil {
		t.Fatalf("read canceled account: %v", err)
	}
	if account.SubscriptionState != SubscriptionStateCanceled || account.TrialEndsAt != nil {
		t.Fatalf("deactivated account state=%q trial_ends_at=%v", account.SubscriptionState, account.TrialEndsAt)
	}
	var tombstones int
	var reason string
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*)::INT, MAX(reason)
FROM billing_organization_tombstones
WHERE org_id = $1`, orgID).Scan(&tombstones, &reason); err != nil {
		t.Fatalf("read billing tombstone: %v", err)
	}
	if tombstones != 1 || reason != "organization_deleted_retry" {
		t.Fatalf("billing tombstone count=%d reason=%q", tombstones, reason)
	}

	// This is the same service method used by delayed
	// organization.plan.changed deliveries. It must fail closed after deletion.
	if err := service.ApplyPlanChange(ctx, orgID, "", "enterprise"); !errors.Is(err, ErrOrganizationDeleted) {
		t.Fatalf("delayed plan event error=%v; want ErrOrganizationDeleted", err)
	}
	if err := service.UpsertAccount(ctx, Account{
		OrgID:             orgID,
		Plan:              "enterprise",
		SubscriptionState: SubscriptionStateActive,
	}); !errors.Is(err, ErrOrganizationDeleted) {
		t.Fatalf("delayed account upsert error=%v; want ErrOrganizationDeleted", err)
	}

	account, err = repo.GetAccount(ctx, orgID)
	if err != nil {
		t.Fatalf("read account after delayed events: %v", err)
	}
	if account.SubscriptionState != SubscriptionStateCanceled || account.Plan != "pro" {
		t.Fatalf("delayed event resurrected account: state=%q plan=%q", account.SubscriptionState, account.Plan)
	}

	failureOrgID := "billing-lifecycle-tombstone-rollback"
	if err := service.UpsertAccount(ctx, Account{
		OrgID:             failureOrgID,
		Plan:              "pro",
		SubscriptionState: SubscriptionStateActive,
	}); err != nil {
		t.Fatalf("seed tombstone rollback account: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_billing_tombstone() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.org_id = 'billing-lifecycle-tombstone-rollback' THEN
    RAISE EXCEPTION 'fixture rejects billing tombstone';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_billing_tombstone
BEFORE INSERT OR UPDATE ON billing_organization_tombstones
FOR EACH ROW EXECUTE FUNCTION reject_fixture_billing_tombstone()`); err != nil {
		t.Fatalf("install tombstone failure fixture: %v", err)
	}
	defer func() {
		_, _ = db.Pool.Exec(context.Background(), `
DROP TRIGGER IF EXISTS reject_fixture_billing_tombstone ON billing_organization_tombstones;
DROP FUNCTION IF EXISTS reject_fixture_billing_tombstone()`)
	}()
	if err := service.DeactivateOrganization(ctx, failureOrgID, "fixture_failure"); err == nil {
		t.Fatal("fixture tombstone failure was swallowed")
	}
	tombstoned, err := repo.IsOrganizationTombstoned(ctx, failureOrgID)
	if err != nil {
		t.Fatalf("check rollback tombstone: %v", err)
	}
	failedAccount, err := repo.GetAccount(ctx, failureOrgID)
	if err != nil {
		t.Fatalf("read account after tombstone rollback: %v", err)
	}
	if tombstoned || failedAccount.SubscriptionState != SubscriptionStateActive {
		t.Fatalf("failed tombstone partially committed tombstoned=%t state=%q", tombstoned, failedAccount.SubscriptionState)
	}

	deactivationFailureOrgID := "billing-lifecycle-deactivation-rollback"
	if err := service.UpsertAccount(ctx, Account{
		OrgID:             deactivationFailureOrgID,
		Plan:              "pro",
		SubscriptionState: SubscriptionStateActive,
	}); err != nil {
		t.Fatalf("seed deactivation rollback account: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_billing_deactivation() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.org_id = 'billing-lifecycle-deactivation-rollback' THEN
    RAISE EXCEPTION 'fixture rejects billing account deactivation';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_billing_deactivation
BEFORE UPDATE ON billing_accounts
FOR EACH ROW EXECUTE FUNCTION reject_fixture_billing_deactivation()`); err != nil {
		t.Fatalf("install deactivation failure fixture: %v", err)
	}
	defer func() {
		_, _ = db.Pool.Exec(context.Background(), `
DROP TRIGGER IF EXISTS reject_fixture_billing_deactivation ON billing_accounts;
DROP FUNCTION IF EXISTS reject_fixture_billing_deactivation()`)
	}()
	if err := repo.TombstoneOrganization(ctx, deactivationFailureOrgID, "fixture_failure"); err == nil {
		t.Fatal("fixture account deactivation failure was swallowed")
	}
	tombstoned, err = repo.IsOrganizationTombstoned(ctx, deactivationFailureOrgID)
	if err != nil {
		t.Fatalf("check deactivation rollback tombstone: %v", err)
	}
	failedAccount, err = repo.GetAccount(ctx, deactivationFailureOrgID)
	if err != nil {
		t.Fatalf("read account after deactivation rollback: %v", err)
	}
	if tombstoned || failedAccount.SubscriptionState != SubscriptionStateActive {
		t.Fatalf("failed deactivation partially committed tombstoned=%t state=%q", tombstoned, failedAccount.SubscriptionState)
	}
}

func TestControlLifecycleBillingPlanRevisionRejectsReorderingAndDuplicates(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "billing_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply billing-core migrations: %v", err)
	}

	repo := NewRepository(db.Pool)
	service := NewService(repo, nil, nil)
	service.orgServiceURL = ""
	sharedPublisher := &lifecycleSharedPublisher{planFailures: 1}
	service.SetSharedPublisher(sharedPublisher)
	canceledCtx, cancelOperation := context.WithCancel(ctx)
	cancelOperation()
	if applied, err := repo.ApplyOrganizationPlanRevision(canceledCtx, Account{
		OrgID:             "billing-plan-revision-canceled",
		Plan:              "pro",
		SubscriptionState: SubscriptionStateActive,
	}, 1); err == nil || applied {
		t.Fatalf("canceled revision apply: applied=%t err=%v", applied, err)
	}
	if _, err := db.Pool.Exec(ctx, `
ALTER TABLE billing_organization_tombstones
RENAME TO billing_organization_tombstones_fixture_unavailable`); err != nil {
		t.Fatalf("hide tombstone table for failure fixture: %v", err)
	}
	tombstoneLookupApplied, tombstoneLookupErr := repo.ApplyOrganizationPlanRevision(ctx, Account{
		OrgID:             "billing-plan-revision-tombstone-lookup-failure",
		Plan:              "pro",
		SubscriptionState: SubscriptionStateActive,
	}, 1)
	if _, err := db.Pool.Exec(ctx, `
ALTER TABLE billing_organization_tombstones_fixture_unavailable
RENAME TO billing_organization_tombstones`); err != nil {
		t.Fatalf("restore tombstone table after failure fixture: %v", err)
	}
	if tombstoneLookupErr == nil || tombstoneLookupApplied {
		t.Fatalf(
			"missing tombstone table revision apply: applied=%t err=%v",
			tombstoneLookupApplied, tombstoneLookupErr,
		)
	}
	revisionFailureOrgID := "billing-plan-revision-write-rollback"
	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_billing_plan_revision() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.org_id = 'billing-plan-revision-write-rollback' THEN
    RAISE EXCEPTION 'fixture rejects billing plan revision';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_billing_plan_revision
BEFORE INSERT OR UPDATE ON billing_accounts
FOR EACH ROW EXECUTE FUNCTION reject_fixture_billing_plan_revision()`); err != nil {
		t.Fatalf("install plan revision failure fixture: %v", err)
	}
	defer func() {
		_, _ = db.Pool.Exec(context.Background(), `
DROP TRIGGER IF EXISTS reject_fixture_billing_plan_revision ON billing_accounts;
DROP FUNCTION IF EXISTS reject_fixture_billing_plan_revision()`)
	}()
	if applied, err := repo.ApplyOrganizationPlanRevision(ctx, Account{
		OrgID:             revisionFailureOrgID,
		Plan:              "pro",
		SubscriptionState: SubscriptionStateActive,
	}, 1); err == nil || applied {
		t.Fatalf("fixture revision write failure: applied=%t err=%v", applied, err)
	}
	var revisionFailureRows int
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*)::INT FROM billing_accounts WHERE org_id = $1`, revisionFailureOrgID).Scan(
		&revisionFailureRows,
	); err != nil {
		t.Fatalf("read revision rollback account count: %v", err)
	}
	if revisionFailureRows != 0 {
		t.Fatalf("failed revision write partially committed %d accounts", revisionFailureRows)
	}
	orgID := "billing-plan-revision-lifecycle"
	if err := service.UpsertAccount(ctx, Account{
		OrgID: orgID, Plan: "free", SubscriptionState: SubscriptionStateActive,
	}); err != nil {
		t.Fatalf("seed isolated billing account: %v", err)
	}

	applied, err := service.ApplyOrganizationPlanChange(ctx, orgID, "Revision Fixture", "enterprise", 2)
	if err == nil || !applied {
		t.Fatalf("first newer plan publish failure: applied=%t err=%v", applied, err)
	}
	applied, err = service.ApplyOrganizationPlanChange(ctx, orgID, "Revision Fixture", "enterprise", 2)
	if err != nil || applied {
		t.Fatalf("retry committed plan revision: applied=%t err=%v", applied, err)
	}
	applied, err = service.ApplyOrganizationPlanChange(ctx, orgID, "Revision Fixture", "pro", 1)
	if err != nil || applied {
		t.Fatalf("apply delayed older revision: applied=%t err=%v", applied, err)
	}
	applied, err = service.ApplyOrganizationPlanChange(ctx, orgID, "Revision Fixture", "standard", 2)
	if err == nil || applied {
		t.Fatalf("conflicting duplicate revision: applied=%t err=%v", applied, err)
	}
	if len(sharedPublisher.revisions) != 2 || sharedPublisher.revisions[0] != 2 || sharedPublisher.revisions[1] != 2 {
		t.Fatalf("shared retry revisions=%v; want stable [2 2]", sharedPublisher.revisions)
	}

	account, err := repo.GetAccount(ctx, orgID)
	if err != nil {
		t.Fatalf("read revisioned billing account: %v", err)
	}
	if account.Plan != "enterprise" || account.PlanRevision != 2 {
		t.Fatalf("reordered account plan=%q revision=%d; want enterprise/2", account.Plan, account.PlanRevision)
	}

	if err := service.DeactivateOrganization(ctx, orgID, "organization_deleted"); err != nil {
		t.Fatalf("tombstone revisioned account: %v", err)
	}
	applied, err = service.ApplyOrganizationPlanChange(ctx, orgID, "Revision Fixture", "hobby", 3)
	if !errors.Is(err, ErrOrganizationDeleted) || applied {
		t.Fatalf("post-tombstone plan revision: applied=%t err=%v", applied, err)
	}
	account, err = repo.GetAccount(ctx, orgID)
	if err != nil {
		t.Fatalf("read tombstoned revisioned account: %v", err)
	}
	if account.Plan != "enterprise" || account.PlanRevision != 2 || account.SubscriptionState != SubscriptionStateCanceled {
		t.Fatalf("tombstoned account plan=%q revision=%d state=%q", account.Plan, account.PlanRevision, account.SubscriptionState)
	}
}

func TestControlLifecycleBillingUsageIsAtomicAndRetrySafe(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "billing_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply billing-core migrations: %v", err)
	}

	repo := NewRepository(db.Pool)
	adapter := &idempotentUsageFixtureAdapter{}
	service := NewService(repo, nil, adapter)
	publisher := &usageFixturePublisher{}
	service.SetPublisher(publisher)
	usage := UsageEvent{
		EventID:    "usage_atomic_fixture_01",
		OrgID:      "billing-usage-atomic",
		Metric:     "api_calls",
		Quantity:   7,
		Source:     "lifecycle-fixture",
		OccurredAt: time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC),
		Metadata:   map[string]interface{}{"fixture": true},
	}
	canceledCtx, cancelRecord := context.WithCancel(ctx)
	cancelRecord()
	if err := service.RecordUsage(canceledCtx, usage); err == nil {
		t.Fatal("canceled usage transaction unexpectedly succeeded")
	}
	if _, err := db.Pool.Exec(ctx, `
ALTER TABLE billing_usage_dedup RENAME TO billing_usage_dedup_fixture_unavailable`); err != nil {
		t.Fatalf("hide usage dedupe table: %v", err)
	}
	dedupeUnavailableErr := service.RecordUsage(ctx, usage)
	if _, err := db.Pool.Exec(ctx, `
ALTER TABLE billing_usage_dedup_fixture_unavailable RENAME TO billing_usage_dedup`); err != nil {
		t.Fatalf("restore usage dedupe table: %v", err)
	}
	if dedupeUnavailableErr == nil {
		t.Fatal("missing usage dedupe table was ignored")
	}

	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_usage_insert() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.org_id = 'billing-usage-atomic' THEN
    RAISE EXCEPTION 'fixture rejects usage insert';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_usage_insert
BEFORE INSERT ON billing_usage_events
FOR EACH ROW EXECUTE FUNCTION reject_fixture_usage_insert()`); err != nil {
		t.Fatalf("install usage failure fixture: %v", err)
	}
	if err := service.RecordUsage(ctx, usage); err == nil {
		t.Fatal("usage insert fixture failure was swallowed")
	}
	if _, err := db.Pool.Exec(ctx, `
DROP TRIGGER reject_fixture_usage_insert ON billing_usage_events;
DROP FUNCTION reject_fixture_usage_insert()`); err != nil {
		t.Fatalf("remove usage failure fixture: %v", err)
	}
	assertUsageRows(t, ctx, db.Pool, usage.EventID, 0, 0, 0)

	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_usage_job() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.dedupe_key = 'lago_usage_report:usage_atomic_fixture_01' THEN
    RAISE EXCEPTION 'fixture rejects usage retry job';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_usage_job
BEFORE INSERT ON billing_retry_jobs
FOR EACH ROW EXECUTE FUNCTION reject_fixture_usage_job()`); err != nil {
		t.Fatalf("install retry-job failure fixture: %v", err)
	}
	if err := service.RecordUsage(ctx, usage); err == nil {
		t.Fatal("retry-job fixture failure was swallowed")
	}
	if _, err := db.Pool.Exec(ctx, `
DROP TRIGGER reject_fixture_usage_job ON billing_retry_jobs;
DROP FUNCTION reject_fixture_usage_job()`); err != nil {
		t.Fatalf("remove retry-job failure fixture: %v", err)
	}
	assertUsageRows(t, ctx, db.Pool, usage.EventID, 0, 0, 0)

	if err := service.RecordUsage(ctx, usage); err != nil {
		t.Fatalf("record atomic usage: %v", err)
	}
	if err := service.RecordUsage(ctx, usage); err != nil {
		t.Fatalf("repeat atomic usage: %v", err)
	}
	assertUsageRows(t, ctx, db.Pool, usage.EventID, 1, 1, 1)
	if len(publisher.eventIDs) != 1 || publisher.eventIDs[0] != usage.EventID {
		t.Fatalf("usage notification IDs=%v; want exactly one stable event", publisher.eventIDs)
	}
	if calls, unique := adapter.counts(usage.EventID); calls != 0 || unique != 0 {
		t.Fatalf("request path called Lago calls=%d unique=%d", calls, unique)
	}

	conflict := usage
	conflict.Quantity = 11
	if err := service.RecordUsage(ctx, conflict); !errors.Is(err, ErrUsageEventConflict) {
		t.Fatalf("conflicting repeated event error=%v; want ErrUsageEventConflict", err)
	}
	assertUsageRows(t, ctx, db.Pool, usage.EventID, 1, 1, 1)
	used, err := repo.GetMetricUsage(ctx, usage.OrgID, usage.Metric)
	if err != nil {
		t.Fatalf("read idempotent usage aggregate: %v", err)
	}
	if used != usage.Quantity {
		t.Fatalf("usage aggregate=%v; want exactly %v", used, usage.Quantity)
	}

	jobs, err := repo.ClaimDueRetryJobs(ctx, 10)
	if err != nil || len(jobs) != 1 {
		t.Fatalf("claim durable usage job jobs=%d err=%v", len(jobs), err)
	}
	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_usage_success() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.dedupe_key = 'lago_usage_report:usage_atomic_fixture_01' AND NEW.status = 'succeeded' THEN
    RAISE EXCEPTION 'fixture crashes before marking Lago success';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_usage_success
BEFORE UPDATE ON billing_retry_jobs
FOR EACH ROW EXECUTE FUNCTION reject_fixture_usage_success()`); err != nil {
		t.Fatalf("install worker crash fixture: %v", err)
	}
	cfg := RetryProcessorConfig{MaxAttempts: 4, BaseBackoff: time.Millisecond}
	if err := service.processRetryJob(ctx, jobs[0], cfg); err == nil {
		t.Fatal("worker success-mark crash fixture was swallowed")
	}
	if _, err := db.Pool.Exec(ctx, `
DROP TRIGGER reject_fixture_usage_success ON billing_retry_jobs;
DROP FUNCTION reject_fixture_usage_success();
UPDATE billing_retry_jobs
SET updated_at = NOW() - INTERVAL '10 minutes'
WHERE dedupe_key = 'lago_usage_report:usage_atomic_fixture_01'`); err != nil {
		t.Fatalf("age stranded processing job: %v", err)
	}

	jobs, err = repo.ClaimDueRetryJobs(ctx, 10)
	if err != nil || len(jobs) != 1 {
		t.Fatalf("reclaim stranded usage job jobs=%d err=%v", len(jobs), err)
	}
	if err := service.processRetryJob(ctx, jobs[0], cfg); err != nil {
		t.Fatalf("retry stranded usage job: %v", err)
	}
	if calls, unique := adapter.counts(usage.EventID); calls != 2 || unique != 1 {
		t.Fatalf("Lago retry identity calls=%d unique=%d; want 2 calls/1 transaction", calls, unique)
	}
	assertUsageRows(t, ctx, db.Pool, usage.EventID, 1, 1, 1)
	var status string
	if err := db.Pool.QueryRow(ctx, `
SELECT status FROM billing_retry_jobs
WHERE dedupe_key = 'lago_usage_report:usage_atomic_fixture_01'`).Scan(&status); err != nil {
		t.Fatalf("read usage retry status: %v", err)
	}
	if status != "succeeded" {
		t.Fatalf("usage retry status=%q; want succeeded", status)
	}

	legacyUsage := UsageEvent{
		EventID:    "usage_legacy_fixture_01",
		OrgID:      "billing-usage-legacy",
		Metric:     "tokens",
		Quantity:   12,
		Source:     "legacy-producer",
		OccurredAt: time.Date(2026, 7, 15, 11, 0, 0, 0, time.UTC),
		Metadata:   map[string]interface{}{"legacy": true},
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO billing_usage_dedup (event_id, org_id, metric, occurred_at)
VALUES ($1, $2, $3, $4)`,
		legacyUsage.EventID, legacyUsage.OrgID, legacyUsage.Metric, legacyUsage.OccurredAt,
	); err != nil {
		t.Fatalf("seed pre-0007 usage reservation: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO billing_usage_events (org_id, metric, quantity, source, occurred_at, metadata)
VALUES ($1, $2, $3, $4, $5, $6::jsonb)`,
		legacyUsage.OrgID, legacyUsage.Metric, legacyUsage.Quantity, legacyUsage.Source,
		legacyUsage.OccurredAt, `{"legacy":true}`,
	); err != nil {
		t.Fatalf("seed pre-0007 usage aggregate: %v", err)
	}
	if err := service.RecordUsage(ctx, legacyUsage); err != nil {
		t.Fatalf("reconcile pre-0007 usage retry: %v", err)
	}
	assertUsageRows(t, ctx, db.Pool, legacyUsage.EventID, 1, 1, 1)
	var legacyAggregateRows int
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*)::INT FROM billing_usage_events
WHERE org_id = $1 AND metric = $2 AND occurred_at = $3`,
		legacyUsage.OrgID, legacyUsage.Metric, legacyUsage.OccurredAt,
	).Scan(&legacyAggregateRows); err != nil {
		t.Fatalf("count reconciled legacy usage: %v", err)
	}
	if legacyAggregateRows != 1 {
		t.Fatalf("legacy usage retry double-counted aggregate rows=%d", legacyAggregateRows)
	}

	legacyConflict := legacyUsage
	legacyConflict.EventID = "usage_legacy_conflict_01"
	legacyConflict.OrgID = "billing-usage-legacy-conflict"
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO billing_usage_dedup (event_id, org_id, metric, occurred_at)
VALUES ($1, $2, $3, $4)`,
		legacyConflict.EventID, legacyConflict.OrgID, legacyConflict.Metric, legacyConflict.OccurredAt,
	); err != nil {
		t.Fatalf("seed conflicting legacy reservation: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO billing_usage_events (org_id, metric, quantity, source, occurred_at, metadata)
VALUES ($1, $2, $3, $4, $5, $6::jsonb)`,
		legacyConflict.OrgID, legacyConflict.Metric, legacyConflict.Quantity+1,
		legacyConflict.Source, legacyConflict.OccurredAt, `{"legacy":true}`,
	); err != nil {
		t.Fatalf("seed conflicting legacy aggregate: %v", err)
	}
	if err := service.RecordUsage(ctx, legacyConflict); !errors.Is(err, ErrUsageEventConflict) {
		t.Fatalf("legacy payload conflict error=%v; want ErrUsageEventConflict", err)
	}

	ambiguousLegacy := legacyUsage
	ambiguousLegacy.EventID = "usage_legacy_ambiguous_01"
	ambiguousLegacy.OrgID = "billing-usage-legacy-ambiguous"
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO billing_usage_dedup (event_id, org_id, metric, occurred_at)
VALUES ($1, $2, $3, $4)`,
		ambiguousLegacy.EventID, ambiguousLegacy.OrgID, ambiguousLegacy.Metric, ambiguousLegacy.OccurredAt,
	); err != nil {
		t.Fatalf("seed ambiguous legacy reservation: %v", err)
	}
	for index := 0; index < 2; index++ {
		if _, err := db.Pool.Exec(ctx, `
INSERT INTO billing_usage_events (org_id, metric, quantity, source, occurred_at, metadata)
VALUES ($1, $2, $3, $4, $5, $6::jsonb)`,
			ambiguousLegacy.OrgID, ambiguousLegacy.Metric, ambiguousLegacy.Quantity,
			ambiguousLegacy.Source, ambiguousLegacy.OccurredAt, `{"legacy":true}`,
		); err != nil {
			t.Fatalf("seed ambiguous legacy aggregate %d: %v", index, err)
		}
	}
	if err := service.RecordUsage(ctx, ambiguousLegacy); !errors.Is(err, ErrUsageEventConflict) {
		t.Fatalf("ambiguous legacy usage error=%v; want ErrUsageEventConflict", err)
	}
}

func assertUsageRows(
	t *testing.T,
	ctx context.Context,
	pool *pgxpool.Pool,
	eventID string,
	wantDedupe, wantUsage, wantJobs int,
) {
	t.Helper()
	var dedupe, usage, jobs int
	if err := pool.QueryRow(ctx, `
SELECT
  (SELECT COUNT(*)::INT FROM billing_usage_dedup WHERE event_id = $1),
  (SELECT COUNT(*)::INT FROM billing_usage_events WHERE event_id = $1),
  (SELECT COUNT(*)::INT FROM billing_retry_jobs WHERE dedupe_key = 'lago_usage_report:' || $1)`, eventID).Scan(
		&dedupe, &usage, &jobs,
	); err != nil {
		t.Fatalf("read atomic usage rows: %v", err)
	}
	if dedupe != wantDedupe || usage != wantUsage || jobs != wantJobs {
		t.Fatalf(
			"atomic usage rows dedupe=%d usage=%d jobs=%d; want %d/%d/%d",
			dedupe, usage, jobs, wantDedupe, wantUsage, wantJobs,
		)
	}
}

func TestControlLifecycleBillingStaleWriterCannotRegressCanonicalRevision(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "billing_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply billing-core migrations: %v", err)
	}

	repo := NewRepository(db.Pool)
	canceledCtx, cancelSave := context.WithCancel(ctx)
	cancelSave()
	if err := repo.SaveAccountStateCAS(canceledCtx, Account{
		OrgID:             "billing-cas-canceled",
		Plan:              "free",
		SubscriptionState: SubscriptionStateActive,
	}); err == nil {
		t.Fatal("canceled account CAS unexpectedly succeeded")
	}
	if _, err := db.Pool.Exec(ctx, `
ALTER TABLE billing_organization_tombstones
RENAME TO billing_organization_tombstones_cas_fixture_unavailable`); err != nil {
		t.Fatalf("hide CAS tombstone table: %v", err)
	}
	tombstoneLookupErr := repo.SaveAccountStateCAS(ctx, Account{
		OrgID:             "billing-cas-tombstone-unavailable",
		Plan:              "free",
		SubscriptionState: SubscriptionStateActive,
	})
	if _, err := db.Pool.Exec(ctx, `
ALTER TABLE billing_organization_tombstones_cas_fixture_unavailable
RENAME TO billing_organization_tombstones`); err != nil {
		t.Fatalf("restore CAS tombstone table: %v", err)
	}
	if tombstoneLookupErr == nil {
		t.Fatal("missing CAS tombstone table was ignored")
	}
	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_billing_cas_insert() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.org_id = 'billing-cas-insert-failure' THEN
    RAISE EXCEPTION 'fixture rejects account CAS insert';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_billing_cas_insert
BEFORE INSERT ON billing_accounts
FOR EACH ROW EXECUTE FUNCTION reject_fixture_billing_cas_insert()`); err != nil {
		t.Fatalf("install account CAS insert failure: %v", err)
	}
	if err := repo.SaveAccountStateCAS(ctx, Account{
		OrgID:             "billing-cas-insert-failure",
		Plan:              "free",
		SubscriptionState: SubscriptionStateActive,
	}); err == nil {
		t.Fatal("account CAS insert failure was swallowed")
	}
	if _, err := db.Pool.Exec(ctx, `
DROP TRIGGER reject_fixture_billing_cas_insert ON billing_accounts;
DROP FUNCTION reject_fixture_billing_cas_insert()`); err != nil {
		t.Fatalf("remove account CAS insert failure: %v", err)
	}
	orgID := "billing-stale-writer-fixture"
	stale := Account{
		OrgID:              orgID,
		Plan:               "free",
		PlanRevision:       1,
		SubscriptionState:  SubscriptionStateTrialing,
		Credits:            10,
		Products:           map[string]bool{"free_product": true},
		FeatureFlags:       map[string]bool{"free_flag": true},
		Entitlements:       map[string]bool{"free_entitlement": true},
		QuotaLimits:        map[string]float64{"api_calls": 100},
		ProviderCustomerID: map[string]string{"payment": "provider-old"},
		Metadata:           map[string]interface{}{"revision": "one"},
	}
	if applied, err := repo.ApplyOrganizationPlanRevision(ctx, stale, 1); err != nil || !applied {
		t.Fatalf("seed revision one applied=%t err=%v", applied, err)
	}

	lockTx, err := db.Pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin canonical revision fixture: %v", err)
	}
	defer func() { _ = lockTx.Rollback(context.Background()) }()
	if _, err := lockTx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtextextended($1, 0))`, orgID); err != nil {
		t.Fatalf("lock canonical revision fixture: %v", err)
	}
	if _, err := lockTx.Exec(ctx, `
UPDATE billing_accounts
SET plan = 'enterprise',
    plan_revision = 2,
    subscription_state = 'active',
    credits = 500,
    products = '{"enterprise_product":true}'::jsonb,
    feature_flags = '{"enterprise_flag":true}'::jsonb,
    entitlements = '{"enterprise_entitlement":true}'::jsonb,
    quota_limits = '{"api_calls":10000}'::jsonb,
    metadata = '{"revision":"two"}'::jsonb,
    trial_ends_at = NULL
WHERE org_id = $1`, orgID); err != nil {
		t.Fatalf("write uncommitted revision two: %v", err)
	}

	stale.ProviderCustomerID = map[string]string{"payment": "provider-new"}
	staleResult := make(chan error, 1)
	go func() {
		staleResult <- repo.SaveAccountStateCAS(context.Background(), stale)
	}()
	select {
	case err := <-staleResult:
		t.Fatalf("stale writer bypassed lifecycle lock: %v", err)
	case <-time.After(100 * time.Millisecond):
	}
	if err := lockTx.Commit(ctx); err != nil {
		t.Fatalf("commit canonical revision two: %v", err)
	}
	select {
	case err := <-staleResult:
		if !errors.Is(err, ErrStaleAccountWrite) {
			t.Fatalf("stale account write error=%v; want ErrStaleAccountWrite", err)
		}
	case <-ctx.Done():
		t.Fatalf("stale writer did not resume: %v", ctx.Err())
	}

	account, err := repo.GetAccount(ctx, orgID)
	if err != nil {
		t.Fatalf("read account after concurrent writers: %v", err)
	}
	if account.Plan != "enterprise" || account.PlanRevision != 2 || account.SubscriptionState != SubscriptionStateActive {
		t.Fatalf("stale writer regressed canonical plan: plan=%q revision=%d state=%q", account.Plan, account.PlanRevision, account.SubscriptionState)
	}
	if account.Credits != 500 || !account.Products["enterprise_product"] || !account.FeatureFlags["enterprise_flag"] ||
		!account.Entitlements["enterprise_entitlement"] || account.QuotaLimits["api_calls"] != 10_000 ||
		account.Metadata["revision"] != "two" {
		t.Fatalf("stale writer regressed revision-owned state: %+v", account)
	}
	if account.ProviderCustomerID["payment"] != "provider-old" {
		t.Fatalf("stale provider snapshot was applied: providers=%v", account.ProviderCustomerID)
	}
	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_billing_cas_update() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.org_id = 'billing-stale-writer-fixture' THEN
    RAISE EXCEPTION 'fixture rejects account CAS update';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_billing_cas_update
BEFORE UPDATE ON billing_accounts
FOR EACH ROW EXECUTE FUNCTION reject_fixture_billing_cas_update()`); err != nil {
		t.Fatalf("install account CAS update failure: %v", err)
	}
	if err := repo.SaveAccountStateCAS(ctx, account); err == nil {
		t.Fatal("account CAS update failure was swallowed")
	}
	if _, err := db.Pool.Exec(ctx, `
DROP TRIGGER reject_fixture_billing_cas_update ON billing_accounts;
DROP FUNCTION reject_fixture_billing_cas_update()`); err != nil {
		t.Fatalf("remove account CAS update failure: %v", err)
	}
	if err := repo.SaveAccountStateCAS(ctx, stale); !errors.Is(err, ErrStaleAccountWrite) {
		t.Fatalf("unversioned stale write error=%v; want ErrStaleAccountWrite", err)
	}

	tombstonedOrgID := "billing-stale-writer-tombstoned"
	if err := repo.SaveAccountStateCAS(ctx, Account{
		OrgID:             tombstonedOrgID,
		Plan:              "free",
		SubscriptionState: SubscriptionStateActive,
	}); err != nil {
		t.Fatalf("seed CAS tombstone account: %v", err)
	}
	if err := repo.TombstoneOrganization(ctx, tombstonedOrgID, "fixture"); err != nil {
		t.Fatalf("tombstone CAS fixture account: %v", err)
	}
	if err := repo.SaveAccountStateCAS(ctx, Account{
		OrgID:             tombstonedOrgID,
		Plan:              "enterprise",
		SubscriptionState: SubscriptionStateActive,
	}); !errors.Is(err, ErrOrganizationDeleted) {
		t.Fatalf("tombstoned CAS write error=%v; want ErrOrganizationDeleted", err)
	}

	firstSameRevision := cloneAccount(account)
	secondSameRevision := cloneAccount(account)
	firstSameRevision.SubscriptionState = SubscriptionStatePastDue
	firstSameRevision.Credits = 450
	firstSameRevision.Metadata["same_revision_writer"] = "first"
	if err := repo.SaveAccountStateCAS(ctx, firstSameRevision); err != nil {
		t.Fatalf("apply current same-revision state: %v", err)
	}
	secondSameRevision.Plan = "free"
	secondSameRevision.SubscriptionState = SubscriptionStateCanceled
	secondSameRevision.Credits = 0
	secondSameRevision.ProviderCustomerID["payment"] = "provider-stale-same-revision"
	secondSameRevision.Metadata["same_revision_writer"] = "stale"
	if err := repo.SaveAccountStateCAS(ctx, secondSameRevision); !errors.Is(err, ErrStaleAccountWrite) {
		t.Fatalf("same-revision stale account error=%v; want ErrStaleAccountWrite", err)
	}
	account, err = repo.GetAccount(ctx, orgID)
	if err != nil {
		t.Fatalf("read same-revision CAS account: %v", err)
	}
	if account.Plan != "enterprise" || account.PlanRevision != 2 ||
		account.SubscriptionState != SubscriptionStatePastDue || account.Credits != 450 ||
		account.Metadata["same_revision_writer"] != "first" ||
		account.ProviderCustomerID["payment"] != "provider-old" {
		t.Fatalf("same-revision stale writer regressed account: %+v", account)
	}
}
