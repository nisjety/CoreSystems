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

type planChangeTestPublisher struct {
	err    error
	events []map[string]any
}

type sharedPlanChangeTestPublisher struct {
	err     error
	eventID string
}

func (p *sharedPlanChangeTestPublisher) PublishOrgCreated(context.Context, string, string, string, string, map[string]any) {
}
func (p *sharedPlanChangeTestPublisher) PublishOrgUpdated(context.Context, string, map[string]any) {
}
func (p *sharedPlanChangeTestPublisher) PublishOrgDeleted(context.Context, string, string) {}
func (p *sharedPlanChangeTestPublisher) PublishPlanChanged(_ context.Context, orgID, _ string, _ string, _ string, _ string, _ string, revision int64) error {
	p.eventID = fmt.Sprintf("organization-plan:%s:%d", orgID, revision)
	return p.err
}
func (p *sharedPlanChangeTestPublisher) PublishInteractiveRetentionEnabled(context.Context, string, int64) error {
	return p.err
}
func (p *sharedPlanChangeTestPublisher) PublishMemberAdded(context.Context, string, string, string, string, string) {
}
func (p *sharedPlanChangeTestPublisher) PublishMemberRemoved(context.Context, string, string) {}
func (p *sharedPlanChangeTestPublisher) PublishPlain(string, map[string]any)                  {}

func (p *planChangeTestPublisher) Publish(_ context.Context, subject string, data map[string]any) error {
	if p.err != nil {
		return p.err
	}
	if subject == "organization.plan.changed" {
		copied := make(map[string]any, len(data))
		for key, value := range data {
			copied[key] = value
		}
		p.events = append(p.events, copied)
	}
	return nil
}

func TestControlLifecyclePlanRevisionOutboxRetriesWithoutLosingOrder(t *testing.T) {
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
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	repo := NewRepository(db)
	orgID := "org-plan-outbox-lifecycle"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: orgID, Name: "Plan Outbox", Plan: "free", Status: "active",
	}, "owner-plan-outbox-lifecycle"); err != nil {
		t.Fatalf("seed organization: %v", err)
	}

	transient := errors.New("fixture publish failure")
	publisher := &planChangeTestPublisher{err: transient}
	service := NewService(repo, publisher)
	sharedPublisher := &sharedPlanChangeTestPublisher{}
	service.SetSharedPublisher(sharedPublisher)
	if _, err := service.UpdatePlan(ctx, orgID, "pro", "user-plan", "upgrade"); !errors.Is(err, transient) {
		t.Fatalf("initial plan publish error=%v; want fixture failure", err)
	}

	var plan string
	var revision, pending, attempts int64
	if err := db.Pool.QueryRow(ctx, `
SELECT o.plan, o.plan_revision,
       COUNT(*) FILTER (WHERE x.published_at IS NULL),
       COALESCE(MAX(x.attempts), 0)
FROM organizations o
JOIN organization_plan_change_outbox x ON x.org_id = o.id
WHERE o.id = $1
GROUP BY o.plan, o.plan_revision`, orgID).Scan(&plan, &revision, &pending, &attempts); err != nil {
		t.Fatalf("read committed plan outbox state: %v", err)
	}
	if plan != "pro" || revision != 1 || pending != 1 || attempts != 1 {
		t.Fatalf("failed publish state plan=%q revision=%d pending=%d attempts=%d", plan, revision, pending, attempts)
	}

	publisher.err = nil
	if published, err := service.FlushPlanChangeOutbox(ctx, 10); err != nil || published != 1 {
		t.Fatalf("retry plan outbox: published=%d err=%v", published, err)
	}
	if len(publisher.events) != 1 || publisher.events[0]["revision"] != int64(1) {
		t.Fatalf("retried event=%v; want revision 1", publisher.events)
	}
	if sharedPublisher.eventID != "organization-plan:"+orgID+":1" {
		t.Fatalf("shared plan event id=%q; want stable revision id", sharedPublisher.eventID)
	}

	sharedTransient := errors.New("fixture shared PubAck failure")
	sharedPublisher.err = sharedTransient
	if _, err := service.UpdatePlan(ctx, orgID, "enterprise", "user-plan", "upgrade-again"); !errors.Is(err, sharedTransient) {
		t.Fatalf("second plan shared publish error=%v; want fixture failure", err)
	}
	var secondPending, secondAttempts int64
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FILTER (WHERE published_at IS NULL), COALESCE(MAX(attempts), 0)
FROM organization_plan_change_outbox WHERE org_id = $1 AND revision = 2`, orgID).Scan(
		&secondPending, &secondAttempts,
	); err != nil {
		t.Fatalf("read shared publish failure state: %v", err)
	}
	if secondPending != 1 || secondAttempts != 1 {
		t.Fatalf("shared failure pending=%d attempts=%d; want 1/1", secondPending, secondAttempts)
	}
	sharedPublisher.err = nil
	if published, err := service.FlushPlanChangeOutbox(ctx, 10); err != nil || published != 1 {
		t.Fatalf("retry shared plan outbox: published=%d err=%v", published, err)
	}
	if _, err := service.UpdatePlan(ctx, orgID, "enterprise", "user-plan", "duplicate"); err != nil {
		t.Fatalf("duplicate plan update: %v", err)
	}

	var revisions []int64
	rows, err := db.Pool.Query(ctx, `
SELECT revision FROM organization_plan_change_outbox
WHERE org_id = $1 ORDER BY revision`, orgID)
	if err != nil {
		t.Fatalf("query outbox revisions: %v", err)
	}
	defer rows.Close()
	for rows.Next() {
		var outboxRevision int64
		if err := rows.Scan(&outboxRevision); err != nil {
			t.Fatalf("scan outbox revision: %v", err)
		}
		revisions = append(revisions, outboxRevision)
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("iterate outbox revisions: %v", err)
	}
	if len(revisions) != 2 || revisions[0] != 1 || revisions[1] != 2 {
		t.Fatalf("outbox revisions=%v; want [1 2]", revisions)
	}

	if _, applied, err := repo.UpdatePlanWithOutbox(
		ctx, "org-plan-outbox-missing", "pro", "user-plan", "missing",
	); !errors.Is(err, ErrNotFound) || applied {
		t.Fatalf("missing organization plan update: applied=%t err=%v", applied, err)
	}

	overflowOrgID := "org-plan-outbox-revision-overflow"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: overflowOrgID, Name: "Plan Overflow", Plan: "free", Status: "active",
	}, "owner-plan-outbox-revision-overflow"); err != nil {
		t.Fatalf("seed revision overflow organization: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
UPDATE organizations SET plan_revision = 9223372036854775807 WHERE id = $1`, overflowOrgID); err != nil {
		t.Fatalf("seed maximum plan revision: %v", err)
	}
	if _, applied, err := repo.UpdatePlanWithOutbox(
		ctx, overflowOrgID, "pro", "user-plan", "overflow-must-rollback",
	); err == nil || applied || !strings.Contains(err.Error(), "plan revision") {
		t.Fatalf("overflow plan update: applied=%t err=%v", applied, err)
	}
	var overflowPlan string
	var overflowRevision int64
	if err := db.Pool.QueryRow(ctx, `
SELECT plan, plan_revision FROM organizations WHERE id = $1`, overflowOrgID).Scan(
		&overflowPlan, &overflowRevision,
	); err != nil {
		t.Fatalf("read overflow rollback state: %v", err)
	}
	if overflowPlan != "free" || overflowRevision != int64(9223372036854775807) {
		t.Fatalf("overflow partially committed plan=%q revision=%d", overflowPlan, overflowRevision)
	}

	rollbackOrgID := "org-plan-outbox-history-rollback"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: rollbackOrgID, Name: "Plan Rollback", Plan: "free", Status: "active",
	}, "owner-plan-outbox-history-rollback"); err != nil {
		t.Fatalf("seed rollback organization: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `
CREATE OR REPLACE FUNCTION reject_fixture_plan_history() RETURNS trigger
LANGUAGE plpgsql AS $fixture$
BEGIN
  IF NEW.org_id = 'org-plan-outbox-history-rollback' THEN
    RAISE EXCEPTION 'fixture rejects plan history';
  END IF;
  RETURN NEW;
END
$fixture$;
CREATE TRIGGER reject_fixture_plan_history
BEFORE INSERT ON org_plan_history
FOR EACH ROW EXECUTE FUNCTION reject_fixture_plan_history()`); err != nil {
		t.Fatalf("install plan history failure fixture: %v", err)
	}
	defer func() {
		_, _ = db.Pool.Exec(context.Background(), `
DROP TRIGGER IF EXISTS reject_fixture_plan_history ON org_plan_history;
DROP FUNCTION IF EXISTS reject_fixture_plan_history()`)
	}()

	if _, applied, err := repo.UpdatePlanWithOutbox(
		ctx, rollbackOrgID, "pro", "user-plan", "must-rollback",
	); err == nil || applied || !strings.Contains(err.Error(), "plan history") {
		t.Fatalf("history failure plan update: applied=%t err=%v", applied, err)
	}

	var rollbackPlan string
	var rollbackRevision, rollbackOutboxRows int64
	if err := db.Pool.QueryRow(ctx, `
SELECT o.plan, o.plan_revision,
       COUNT(x.*)
FROM organizations o
LEFT JOIN organization_plan_change_outbox x ON x.org_id = o.id
WHERE o.id = $1
GROUP BY o.plan, o.plan_revision`, rollbackOrgID).Scan(
		&rollbackPlan, &rollbackRevision, &rollbackOutboxRows,
	); err != nil {
		t.Fatalf("read rolled-back plan state: %v", err)
	}
	if rollbackPlan != "free" || rollbackRevision != 0 || rollbackOutboxRows != 0 {
		t.Fatalf(
			"failed history write partially committed plan=%q revision=%d outbox_rows=%d",
			rollbackPlan, rollbackRevision, rollbackOutboxRows,
		)
	}
}
