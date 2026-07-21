package org

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

func TestControlLifecycleGDPRAuditOutboxIsAtomicRetryableAndBounded(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	repo := NewRepository(db)
	publisher := &gdprAuditPublisherStub{}
	service := NewService(repo, nil)
	service.SetAuditPublisher(publisher)
	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	seed := func(label string) string {
		t.Helper()
		orgID := "org-gdpr-audit-" + label + "-" + suffix
		if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
			ID: orgID, Name: "GDPR audit " + label, Plan: "free", Status: "active",
		}, "owner-gdpr-audit-"+label+"-"+suffix); err != nil {
			t.Fatalf("seed %s organization: %v", label, err)
		}
		return orgID
	}

	// The state transition and success audit intent are one transaction. Force
	// only the success outbox insert to fail; the soft-delete must roll back and
	// the subsequent independent error audit remains durable.
	atomicOrgID := seed("atomic")
	if _, err := db.Pool.Exec(ctx, `
CREATE TABLE IF NOT EXISTS gdpr_audit_rejection_fixture (org_id TEXT PRIMARY KEY);
CREATE OR REPLACE FUNCTION reject_fixture_gdpr_success_audit() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.payload->>'outcome' = 'ok'
     AND EXISTS (
       SELECT 1 FROM gdpr_audit_rejection_fixture f
       WHERE f.org_id = NEW.payload->>'org_id'
     ) THEN
    RAISE EXCEPTION 'fixture rejects GDPR success audit';
  END IF;
  RETURN NEW;
END
$fixture$;
DROP TRIGGER IF EXISTS reject_fixture_gdpr_success_audit
  ON organization_gdpr_audit_outbox;
CREATE TRIGGER reject_fixture_gdpr_success_audit
BEFORE INSERT ON organization_gdpr_audit_outbox
FOR EACH ROW EXECUTE FUNCTION reject_fixture_gdpr_success_audit()`); err != nil {
		t.Fatalf("install atomicity failure fixture: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
INSERT INTO gdpr_audit_rejection_fixture (org_id) VALUES ($1)
ON CONFLICT (org_id) DO NOTHING`, atomicOrgID); err != nil {
		t.Fatalf("seed atomicity failure fixture: %v", err)
	}
	if _, err := service.SoftDelete(ctx, atomicOrgID, "GDPR audit atomic", "owner-atomic", "owner"); err == nil || !strings.Contains(err.Error(), "success audit") {
		t.Fatalf("soft delete with rejected audit error=%v", err)
	}
	var status string
	var deletedAt *time.Time
	if err := db.Pool.QueryRow(ctx, `SELECT status, deleted_at FROM organizations WHERE id = $1`, atomicOrgID).Scan(&status, &deletedAt); err != nil {
		t.Fatalf("read atomic rollback organization: %v", err)
	}
	if status != "active" || deletedAt != nil {
		t.Fatalf("soft delete committed without audit intent: status=%q deleted_at=%v", status, deletedAt)
	}
	var errorAuditCount int
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_gdpr_audit_outbox
WHERE payload->>'org_id' = $1 AND payload->>'outcome' = 'error'`, atomicOrgID).Scan(&errorAuditCount); err != nil {
		t.Fatalf("read durable error audit: %v", err)
	}
	if errorAuditCount != 1 {
		t.Fatalf("durable error audits=%d; want 1", errorAuditCount)
	}
	if _, err := db.Pool.Exec(ctx, `
DROP TRIGGER reject_fixture_gdpr_success_audit ON organization_gdpr_audit_outbox;
DROP FUNCTION reject_fixture_gdpr_success_audit();
DROP TABLE gdpr_audit_rejection_fixture`); err != nil {
		t.Fatalf("remove atomicity failure fixture: %v", err)
	}
	if result, err := service.FlushGDPRAuditOutbox(ctx, 10); err != nil || result.Published != 1 {
		t.Fatalf("publish durable error audit: result=%+v err=%v", result, err)
	}

	// A transient PubAck failure increments attempts but leaves the row pending.
	// The exact stored event identity must be reused by the successful retry.
	retryOrgID := seed("retry")
	if _, err := service.SoftDelete(ctx, retryOrgID, "GDPR audit retry", "owner-retry", "owner"); err != nil {
		t.Fatalf("soft delete retry fixture: %v", err)
	}
	var retryEventID string
	if err := db.Pool.QueryRow(ctx, `
SELECT event_id FROM organization_gdpr_audit_outbox
WHERE payload->>'org_id' = $1 AND payload->>'outcome' = 'ok'`, retryOrgID).Scan(&retryEventID); err != nil {
		t.Fatalf("read retry event id: %v", err)
	}
	transient := errors.New("fixture PubAck failure")
	publisher.err = transient
	result, err := service.FlushGDPRAuditOutbox(ctx, 10)
	if !errors.Is(err, transient) || result.Published != 0 || result.DeadLettered != 0 {
		t.Fatalf("transient flush result=%+v err=%v", result, err)
	}
	var attempts int
	var lastError string
	var publishedAt, deadLetteredAt *time.Time
	if err := db.Pool.QueryRow(ctx, `
SELECT attempts, last_error, published_at, dead_lettered_at
FROM organization_gdpr_audit_outbox WHERE event_id = $1`, retryEventID).Scan(
		&attempts, &lastError, &publishedAt, &deadLetteredAt,
	); err != nil {
		t.Fatalf("read transient retry state: %v", err)
	}
	if attempts != 1 || !strings.Contains(lastError, transient.Error()) || publishedAt != nil || deadLetteredAt != nil {
		t.Fatalf("transient state attempts=%d error=%q published=%v dead=%v", attempts, lastError, publishedAt, deadLetteredAt)
	}
	if _, err := db.Pool.Exec(ctx, `UPDATE organization_gdpr_audit_outbox SET next_attempt_at = NOW() WHERE event_id = $1`, retryEventID); err != nil {
		t.Fatalf("make retry due: %v", err)
	}
	publisher.err = nil
	beforeRetryPublishes := len(publisher.eventIDs)
	if result, err := service.FlushGDPRAuditOutbox(ctx, 10); err != nil || result.Published != 1 {
		t.Fatalf("successful retry result=%+v err=%v", result, err)
	}
	if len(publisher.eventIDs) != beforeRetryPublishes+1 || publisher.eventIDs[len(publisher.eventIDs)-1] != retryEventID {
		t.Fatalf("retry event IDs=%v; want final %q", publisher.eventIDs, retryEventID)
	}
	if result, err := service.FlushGDPRAuditOutbox(ctx, 10); err != nil || result.Published != 0 {
		t.Fatalf("published row was reclaimed: result=%+v err=%v", result, err)
	}

	// Repeated failures stop at the bounded retry ceiling and remain visible in
	// the local dead-letter state; terminal rows are never reclaimed.
	deadOrgID := seed("dead")
	if _, err := service.SoftDelete(ctx, deadOrgID, "GDPR audit dead", "owner-dead", "owner"); err != nil {
		t.Fatalf("soft delete dead-letter fixture: %v", err)
	}
	var deadEventID string
	if err := db.Pool.QueryRow(ctx, `
SELECT event_id FROM organization_gdpr_audit_outbox
WHERE payload->>'org_id' = $1 AND payload->>'outcome' = 'ok'`, deadOrgID).Scan(&deadEventID); err != nil {
		t.Fatalf("read dead-letter event id: %v", err)
	}
	publisher.err = transient
	for attempt := 1; attempt <= GDPRAuditMaxAttempts; attempt++ {
		result, err = service.FlushGDPRAuditOutbox(ctx, 10)
		if !errors.Is(err, transient) {
			t.Fatalf("dead-letter attempt %d error=%v", attempt, err)
		}
		wantDead := 0
		if attempt == GDPRAuditMaxAttempts {
			wantDead = 1
		}
		if result.DeadLettered != wantDead {
			t.Fatalf("dead-letter attempt %d result=%+v; want dead=%d", attempt, result, wantDead)
		}
		if attempt < GDPRAuditMaxAttempts {
			if _, err := db.Pool.Exec(ctx, `UPDATE organization_gdpr_audit_outbox SET next_attempt_at = NOW() WHERE event_id = $1`, deadEventID); err != nil {
				t.Fatalf("make dead-letter retry %d due: %v", attempt+1, err)
			}
		}
	}
	publisher.err = nil
	if result, err := service.FlushGDPRAuditOutbox(ctx, 10); err != nil || result.Published != 0 {
		t.Fatalf("dead-letter row was reclaimed: result=%+v err=%v", result, err)
	}
	outboxStatus, err := service.GDPRAuditOutboxStatus(ctx)
	if err != nil {
		t.Fatalf("read GDPR audit outbox status: %v", err)
	}
	if outboxStatus.DeadLettered != 1 {
		t.Fatalf("outbox status=%+v; want one visible dead letter", outboxStatus)
	}

	// Hard erasure must leave the audit intent after the organization and its
	// dependent rows are gone; the outbox deliberately has no org foreign key.
	hardOrgID := seed("hard")
	if _, err := service.HardDelete(ctx, hardOrgID, "owner-hard", "owner"); err != nil {
		t.Fatalf("hard delete with audit outbox: %v", err)
	}
	var hardOrgCount, hardAuditCount int
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM organizations WHERE id = $1`, hardOrgID).Scan(&hardOrgCount); err != nil {
		t.Fatalf("count hard-deleted organization: %v", err)
	}
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_gdpr_audit_outbox
WHERE payload->>'org_id' = $1 AND payload->>'outcome' = 'ok'`, hardOrgID).Scan(&hardAuditCount); err != nil {
		t.Fatalf("count hard-delete audit: %v", err)
	}
	if hardOrgCount != 0 || hardAuditCount != 1 {
		t.Fatalf("hard delete state orgs=%d audits=%d; want 0/1", hardOrgCount, hardAuditCount)
	}

	// Storage and acknowledgement boundaries fail closed without corrupting a
	// pending row or panicking. These calls use only the disposable fixture.
	if _, err := repo.ClaimGDPRAuditOutbox(ctx, 0); err == nil {
		t.Fatal("invalid direct claim limit was accepted")
	}
	if err := repo.MarkGDPRAuditPublished(ctx, "missing-event"); err == nil {
		t.Fatal("nonmatching audit acknowledgement was accepted")
	}
	if _, err := repo.MarkGDPRAuditPublishFailed(ctx, "missing-event", transient); err == nil {
		t.Fatal("nonmatching audit failure was accepted")
	}
	missingOrgID := "org-gdpr-audit-missing-" + suffix
	if _, err := service.SoftDelete(ctx, missingOrgID, "", "owner-missing", "owner"); err == nil || !strings.Contains(err.Error(), "organization erasure failed") {
		t.Fatalf("missing organization semantic failure=%v", err)
	}
	if result, err := service.FlushGDPRAuditOutbox(ctx, 10); err != nil || result.Published != 2 {
		// The missing-org error audit and the hard-erasure success audit are both
		// still pending at this point.
		t.Fatalf("flush final durable audits: result=%+v err=%v", result, err)
	}
	leaseEvent, err := newGDPRAuditEvent(
		"org-lease", "organization_soft", "org-lease",
		"owner-lease", "owner", "ok", time.Now().UTC(),
	)
	if err != nil {
		t.Fatal(err)
	}
	if err := repo.EnqueueGDPRAuditEvent(ctx, leaseEvent); err != nil {
		t.Fatalf("enqueue lease event: %v", err)
	}
	claimed, err := repo.ClaimGDPRAuditOutbox(ctx, 1)
	if err != nil || len(claimed) != 1 || claimed[0].EventID != leaseEvent.EventID {
		t.Fatalf("first lease claim=%v err=%v", claimed, err)
	}
	claimedAgain, err := repo.ClaimGDPRAuditOutbox(ctx, 1)
	if err != nil || len(claimedAgain) != 0 {
		t.Fatalf("active lease was reclaimed: rows=%v err=%v", claimedAgain, err)
	}
	if _, err := db.Pool.Exec(ctx, `
UPDATE organization_gdpr_audit_outbox
SET processing_at = NOW() - INTERVAL '2 minutes'
WHERE event_id = $1`, leaseEvent.EventID); err != nil {
		t.Fatalf("expire fixture lease: %v", err)
	}
	reclaimed, err := repo.ClaimGDPRAuditOutbox(ctx, 1)
	if err != nil || len(reclaimed) != 1 || reclaimed[0].EventID != leaseEvent.EventID {
		t.Fatalf("expired lease reclaim=%v err=%v", reclaimed, err)
	}
	if err := repo.MarkGDPRAuditPublished(ctx, leaseEvent.EventID); err != nil {
		t.Fatalf("complete reclaimed lease: %v", err)
	}

	// The scoped runtime role may observe/update its own outbox state but cannot
	// delete durable evidence. The transaction is isolated and rolled back.
	assertRuntimeMutationDenied := func(label, statement string) {
		t.Helper()
		privilegeTx, err := db.Pool.Begin(ctx)
		if err != nil {
			t.Fatalf("begin %s privilege check: %v", label, err)
		}
		defer func() { _ = privilegeTx.Rollback(ctx) }()
		if _, err := privilegeTx.Exec(ctx, `SELECT set_config('app.current_org', $1, true)`, leaseEvent.OrgID); err != nil {
			t.Fatalf("set %s privilege org scope: %v", label, err)
		}
		if _, err := privilegeTx.Exec(ctx, `SET LOCAL ROLE org_core_app`); err != nil {
			t.Fatalf("set %s privilege runtime role: %v", label, err)
		}
		if _, err := privilegeTx.Exec(ctx, statement, leaseEvent.EventID); err == nil {
			t.Fatalf("runtime role performed forbidden GDPR audit %s", label)
		}
	}
	assertRuntimeMutationDenied("deletion", `
DELETE FROM organization_gdpr_audit_outbox WHERE event_id = $1`)
	assertRuntimeMutationDenied("payload tampering", `
UPDATE organization_gdpr_audit_outbox
SET payload = jsonb_set(payload, '{outcome}', '"forged"'::jsonb)
WHERE event_id = $1`)

	validEvent, err := newGDPRAuditEvent(
		"org-cancelled", "organization_soft", "org-cancelled",
		"owner-cancelled", "owner", "ok", time.Now().UTC(),
	)
	if err != nil {
		t.Fatal(err)
	}
	cancelled, cancelNow := context.WithCancel(context.Background())
	cancelNow()
	if _, err := repo.executeGDPRAuditOperation(
		cancelled, "org-cancelled", `SELECT soft_delete_organization($1)`, validEvent,
	); err == nil {
		t.Fatal("cancelled GDPR operation unexpectedly began")
	}
	if _, err := repo.ClaimGDPRAuditOutbox(cancelled, 1); err == nil {
		t.Fatal("cancelled outbox claim unexpectedly began")
	}
	if _, err := repo.MarkGDPRAuditPublishFailed(cancelled, "missing-event", transient); err == nil {
		t.Fatal("cancelled failure update unexpectedly succeeded")
	}
	if _, err := repo.GDPRAuditOutboxStatus(cancelled); err == nil {
		t.Fatal("cancelled outbox status unexpectedly succeeded")
	}
	if _, err := service.GDPRAuditOutboxStatus(cancelled); err == nil {
		t.Fatal("cancelled service outbox status unexpectedly succeeded")
	}
}
