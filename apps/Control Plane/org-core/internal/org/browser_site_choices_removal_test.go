package org

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/browsersites"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// recordingChoiceEraser records each erase the removal path asks for and answers the
// configured error, so a test can see the failure the caller gets back.
type recordingChoiceEraser struct {
	calls int
	err   error
}

func (e *recordingChoiceEraser) DeleteRemovedMemberData(_ context.Context, _, _ string) (int64, int64, error) {
	e.calls++
	return 1, 1, e.err
}

// TestMembershipRemovalRefusesToRunWithoutTheChoiceEraser pins that a removal refuses
// before it writes anything when the choice eraser is not wired: no membership ends with
// its choices left behind. No database is needed, because the refusal comes first.
func TestMembershipRemovalRefusesToRunWithoutTheChoiceEraser(t *testing.T) {
	service := NewService(nil, nil)
	ctx := context.Background()
	if err := service.RemoveOrganizationMember(ctx, "org-1", "user-1"); !errors.Is(err, errBrowserSiteChoicesUnwired) {
		t.Fatalf("a removal without the eraser must refuse before any write, got %v", err)
	}
	if _, err := service.ReconcileOrganizationMember(ctx, "org-1", "user-1", "", "remove", 3); !errors.Is(err, errBrowserSiteChoicesUnwired) {
		t.Fatalf("a reconcile remove without the eraser must refuse before any write, got %v", err)
	}
}

// TestRemovedMemberChoiceErasureReturnsItsFailureWithoutIds pins that a failed erase is
// returned to the caller, so the caller retries, and that the error names no organisation
// or user. A successful erase returns nil.
func TestRemovedMemberChoiceErasureReturnsItsFailureWithoutIds(t *testing.T) {
	boom := errors.New("database unavailable")
	eraser := &recordingChoiceEraser{err: boom}
	service := NewService(nil, nil)
	service.SetBrowserSiteChoiceEraser(eraser)
	ctx := context.Background()

	err := service.eraseRemovedMemberChoices(ctx, "org-private-1", "user-private-1")
	if !errors.Is(err, boom) {
		t.Fatalf("a failed erase must be returned to the caller, got %v", err)
	}
	if strings.Contains(err.Error(), "org-private-1") || strings.Contains(err.Error(), "user-private-1") {
		t.Fatalf("the erase failure must carry no organisation or user id: %v", err)
	}

	eraser.err = nil
	if err := service.eraseRemovedMemberChoices(ctx, "org-private-1", "user-private-1"); err != nil {
		t.Fatalf("a successful erase must return nil, got %v", err)
	}
	if eraser.calls != 2 {
		t.Fatalf("the eraser must be asked on every call, got %d calls", eraser.calls)
	}
}

// TestControlLifecycleMembershipRemovalErasesTheMembersSiteChoices runs the two removal
// paths against the disposable lifecycle Postgres with the real browser-site repository as
// the eraser. The service removal and the Auth-projection remove both erase the member's
// choice, and a removal whose erase fails returns the failure with the membership ended,
// and a retry finishes the erase.
func TestControlLifecycleMembershipRemovalErasesTheMembersSiteChoices(t *testing.T) {
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
	sites := browsersites.NewRepository(db)
	service := NewService(repo, nil)
	service.SetBrowserSiteChoiceEraser(browsersites.NewService(sites, time.Now))

	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	orgID := "org-membership-erase-" + suffix
	ownerID := "owner-membership-erase-" + suffix
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: orgID, Name: "Membership erase", Plan: "free", Status: "active",
	}, ownerID); err != nil {
		t.Fatalf("provision organization: %v", err)
	}

	countChoices := func(userID string) int {
		t.Helper()
		var count int
		if err := db.Pool.QueryRow(ctx, `SELECT count(*) FROM user_browser_site_prefs WHERE org_id = $1 AND user_id = $2`, orgID, userID).Scan(&count); err != nil {
			t.Fatalf("count choices for a member: %v", err)
		}
		return count
	}
	seedChoice := func(userID string) {
		t.Helper()
		if err := repo.AddOrganizationMember(ctx, orgID, userID, "member"); err != nil {
			t.Fatalf("seed member: %v", err)
		}
		if _, err := sites.PutPref(ctx, orgID, userID, 0, browsersites.Pref{
			Host: "news.example.com", Level: browsersites.LevelAct, ExpiresAt: time.Now().Add(time.Hour),
		}); err != nil {
			t.Fatalf("seed the member's choice: %v", err)
		}
	}

	// The service removal erases the choice.
	removedByService := "member-service-" + suffix
	seedChoice(removedByService)
	if err := service.RemoveOrganizationMember(ctx, orgID, removedByService); err != nil {
		t.Fatalf("service removal: %v", err)
	}
	if n := countChoices(removedByService); n != 0 {
		t.Fatalf("the service removal must erase the member's choices, %d left", n)
	}

	// The Auth-projection remove, the production path, erases the choice too.
	removedByProjection := "member-projection-" + suffix
	seedChoice(removedByProjection)
	if _, err := service.ReconcileOrganizationMember(ctx, orgID, removedByProjection, "", "remove", 7); err != nil {
		t.Fatalf("projection remove: %v", err)
	}
	if n := countChoices(removedByProjection); n != 0 {
		t.Fatalf("the projection remove must erase the member's choices, %d left", n)
	}

	// A failed erase is returned, the membership stays ended, and a retry finishes the erase.
	failing := &recordingChoiceEraser{err: errors.New("erase unavailable")}
	service.SetBrowserSiteChoiceEraser(failing)
	retried := "member-retry-" + suffix
	seedChoice(retried)
	if err := service.RemoveOrganizationMember(ctx, orgID, retried); err == nil {
		t.Fatal("a removal whose erase fails must return the failure")
	}
	var status string
	if err := db.Pool.QueryRow(ctx, `SELECT status FROM organization_members WHERE org_id = $1 AND user_id = $2`, orgID, retried).Scan(&status); err != nil || status != "removed" {
		t.Fatalf("the membership must end even when the erase fails: status=%q err=%v", status, err)
	}
	service.SetBrowserSiteChoiceEraser(browsersites.NewService(sites, time.Now))
	if err := service.RemoveOrganizationMember(ctx, orgID, retried); err != nil {
		t.Fatalf("the retry must finish the removal: %v", err)
	}
	if n := countChoices(retried); n != 0 {
		t.Fatalf("the retry must erase the member's choices, %d left", n)
	}
}
