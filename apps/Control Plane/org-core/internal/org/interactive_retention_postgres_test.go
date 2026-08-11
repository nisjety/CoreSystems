package org

import (
	"context"
	"errors"
	"os"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

type interactiveRetentionOutboxTestPublisher struct {
	err      error
	orgIDs   []string
	eventIDs []int64
}

func (p *interactiveRetentionOutboxTestPublisher) PublishOrgCreated(context.Context, string, string, string, string, map[string]any) {
}
func (p *interactiveRetentionOutboxTestPublisher) PublishOrgUpdated(context.Context, string, map[string]any) {
}
func (p *interactiveRetentionOutboxTestPublisher) PublishOrgDeleted(context.Context, string, string) {
}
func (p *interactiveRetentionOutboxTestPublisher) PublishPlanChanged(context.Context, string, string, string, string, string, string, int64) error {
	return nil
}
func (p *interactiveRetentionOutboxTestPublisher) PublishInteractiveRetentionEnabled(_ context.Context, orgID string, eventID int64) error {
	if p.err != nil {
		return p.err
	}
	p.orgIDs = append(p.orgIDs, orgID)
	p.eventIDs = append(p.eventIDs, eventID)
	return nil
}
func (p *interactiveRetentionOutboxTestPublisher) PublishMemberAdded(context.Context, string, string, string, string, string) {
}
func (p *interactiveRetentionOutboxTestPublisher) PublishMemberRemoved(context.Context, string, string) {
}
func (p *interactiveRetentionOutboxTestPublisher) PublishPlain(string, map[string]any) {}

// TestControlLifecycleInteractiveRetentionOutboxDrainsOnTransition exercises
// SetInteractiveRetention's false-to-true enqueue rule and
// FlushInteractiveRetentionOutbox's claim/publish/retry cycle against a real
// Postgres, mirroring TestControlLifecyclePlanRevisionOutboxRetriesWithoutLosingOrder.
func TestControlLifecycleInteractiveRetentionOutboxDrainsOnTransition(t *testing.T) {
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
	orgID := "org-interactive-retention-lifecycle"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: orgID, Name: "Interactive Retention", Plan: "enterprise", Status: "active",
	}, "owner-interactive-retention-lifecycle"); err != nil {
		t.Fatalf("seed organization: %v", err)
	}

	publisher := &interactiveRetentionOutboxTestPublisher{}
	service := NewService(repo, nil)
	service.SetSharedPublisher(publisher)

	// Enabling for the first time is a false-to-true transition: it must enqueue.
	if _, err := service.SetInteractiveRetention(ctx, orgID, true, "user-1"); err != nil {
		t.Fatalf("enable interactive retention: %v", err)
	}
	var pending int64
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_interactive_retention_outbox
WHERE org_id = $1 AND published_at IS NULL`, orgID).Scan(&pending); err != nil {
		t.Fatalf("read outbox after first enable: %v", err)
	}
	if pending != 1 {
		t.Fatalf("pending after first enable=%d; want 1", pending)
	}

	// Re-enabling an already-true posture is not a transition: no new row.
	if _, err := service.SetInteractiveRetention(ctx, orgID, true, "user-1"); err != nil {
		t.Fatalf("re-enable interactive retention: %v", err)
	}
	var totalAfterReenable int64
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_interactive_retention_outbox WHERE org_id = $1`, orgID).Scan(&totalAfterReenable); err != nil {
		t.Fatalf("read outbox after re-enable: %v", err)
	}
	if totalAfterReenable != 1 {
		t.Fatalf("total outbox rows after re-enable=%d; want 1 (no duplicate)", totalAfterReenable)
	}

	// Disabling never enqueues.
	if _, err := service.SetInteractiveRetention(ctx, orgID, false, "user-1"); err != nil {
		t.Fatalf("disable interactive retention: %v", err)
	}
	var totalAfterDisable int64
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_interactive_retention_outbox WHERE org_id = $1`, orgID).Scan(&totalAfterDisable); err != nil {
		t.Fatalf("read outbox after disable: %v", err)
	}
	if totalAfterDisable != 1 {
		t.Fatalf("total outbox rows after disable=%d; want 1 (disable must not enqueue)", totalAfterDisable)
	}

	// A publish failure leaves the row pending with attempts/last_error recorded.
	publisher.err = errors.New("fixture publish failure")
	if _, err := service.SetInteractiveRetention(ctx, orgID, true, "user-1"); err != nil {
		t.Fatalf("re-enable after disable: %v", err)
	}
	var secondPending int64
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_interactive_retention_outbox
WHERE org_id = $1 AND published_at IS NULL`, orgID).Scan(&secondPending); err != nil {
		t.Fatalf("read outbox after second enable: %v", err)
	}
	if secondPending != 1 {
		t.Fatalf("pending after second enable=%d; want 1", secondPending)
	}
	var attempts int64
	var lastError *string
	if err := db.Pool.QueryRow(ctx, `
SELECT attempts, last_error FROM organization_interactive_retention_outbox
WHERE org_id = $1 AND published_at IS NULL`, orgID).Scan(&attempts, &lastError); err != nil {
		t.Fatalf("read pending row: %v", err)
	}
	if attempts != 1 || lastError == nil || *lastError != "fixture publish failure" {
		t.Fatalf("failed-publish state attempts=%d last_error=%v", attempts, lastError)
	}

	publisher.err = nil
	published, err := service.FlushInteractiveRetentionOutbox(ctx, 10)
	if err != nil || published != 1 {
		t.Fatalf("retry outbox: published=%d err=%v", published, err)
	}
	if len(publisher.orgIDs) != 1 || publisher.orgIDs[0] != orgID {
		t.Fatalf("published org ids=%v; want [%s]", publisher.orgIDs, orgID)
	}
	var finalPending int64
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_interactive_retention_outbox
WHERE org_id = $1 AND published_at IS NULL`, orgID).Scan(&finalPending); err != nil {
		t.Fatalf("read outbox after retry: %v", err)
	}
	if finalPending != 0 {
		t.Fatalf("pending after successful retry=%d; want 0", finalPending)
	}

	// A free-plan org is not entitled: ErrPlanUpgradeRequired, no outbox row.
	freeOrgID := "org-interactive-retention-free-plan"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: freeOrgID, Name: "Free Plan", Plan: "free", Status: "active",
	}, "owner-interactive-retention-free-plan"); err != nil {
		t.Fatalf("seed free-plan organization: %v", err)
	}
	if _, err := service.SetInteractiveRetention(ctx, freeOrgID, true, "user-1"); !errors.Is(err, ErrPlanUpgradeRequired) {
		t.Fatalf("free-plan enable error=%v; want ErrPlanUpgradeRequired", err)
	}
	var freeOutboxRows int64
	if err := db.Pool.QueryRow(ctx, `
SELECT COUNT(*) FROM organization_interactive_retention_outbox WHERE org_id = $1`, freeOrgID).Scan(&freeOutboxRows); err != nil {
		t.Fatalf("read free-plan outbox: %v", err)
	}
	if freeOutboxRows != 0 {
		t.Fatalf("free-plan outbox rows=%d; want 0", freeOutboxRows)
	}

	if _, err := repo.SetInteractiveRetention(ctx, "org-interactive-retention-missing", true, "user-1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("missing organization error=%v; want ErrNotFound", err)
	}
}

// TestControlLifecycleSupportAIModePersists exercises SetSupportAIMode's
// metadata.supportAi.mode round trip, matching the shape
// apps/Frontend Plane/verevonv3's organization-client.ts reads back.
func TestControlLifecycleSupportAIModePersists(t *testing.T) {
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
	orgID := "org-support-ai-mode-lifecycle"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: orgID, Name: "Support AI Mode", Plan: "pro", Status: "active",
	}, "owner-support-ai-mode-lifecycle"); err != nil {
		t.Fatalf("seed organization: %v", err)
	}

	service := NewService(repo, nil)
	orgData, err := service.SetSupportAIMode(ctx, orgID, "Assist", "user-1")
	if err != nil {
		t.Fatalf("set support ai mode: %v", err)
	}
	supportAi, _ := orgData.Metadata["supportAi"].(map[string]any)
	if supportAi == nil || supportAi["mode"] != "assist" {
		t.Fatalf("support ai metadata=%v; want mode=assist", orgData.Metadata["supportAi"])
	}

	if _, err := service.SetSupportAIMode(ctx, orgID, "loud", "user-1"); err == nil {
		t.Fatal("invalid support AI mode was accepted")
	}
	unchanged, err := repo.GetOrganization(ctx, orgID)
	if err != nil {
		t.Fatalf("re-read organization: %v", err)
	}
	unchangedSupportAi, _ := unchanged.Metadata["supportAi"].(map[string]any)
	if unchangedSupportAi == nil || unchangedSupportAi["mode"] != "assist" {
		t.Fatalf("support ai metadata after rejected mode=%v; want unchanged mode=assist", unchanged.Metadata["supportAi"])
	}

	if err := repo.SetSupportAIMode(ctx, "org-support-ai-mode-missing", "off", "user-1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("missing organization error=%v; want ErrNotFound", err)
	}
}
