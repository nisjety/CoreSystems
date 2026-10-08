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

// TestControlLifecycleRetentionPostureReadsStoredStateFromPostgres exercises
// GetRetentionPosture's query against a real Postgres: stored intent and plan
// are read straight from the table, only an exact JSON `true` is intent, a lapsed plan
// keeps the intent but loses the entitlement, and a soft-deleted or unknown org
// is not found. Gated like the other lifecycle tests.
func TestControlLifecycleRetentionPostureReadsStoredStateFromPostgres(t *testing.T) {
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
	service := NewService(repo, nil)
	service.SetSharedPublisher(&interactiveRetentionOutboxTestPublisher{})

	orgID := "org-retention-posture-read"
	if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{
		ID: orgID, Name: "Retention Posture Read", Slug: "retention-posture-read", Plan: "enterprise", Status: "active",
	}, "owner-retention-posture-read"); err != nil {
		t.Fatalf("seed organization: %v", err)
	}

	assertPosture := func(label string, want RetentionPosture) {
		t.Helper()
		got, err := service.GetRetentionPosture(ctx, orgID)
		if err != nil {
			t.Fatalf("%s: read posture: %v", label, err)
		}
		if *got != want {
			t.Fatalf("%s: posture=%+v want=%+v", label, *got, want)
		}
	}

	assertPosture("entitled plan, never switched on", RetentionPosture{ZDRIntent: false, Entitled: true})

	if _, err := service.SetInteractiveRetention(ctx, orgID, true, "user-1"); err != nil {
		t.Fatalf("enable interactive retention: %v", err)
	}
	assertPosture("switched on", RetentionPosture{ZDRIntent: true, Entitled: true})

	if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET plan = 'free' WHERE id = $1`, orgID); err != nil {
		t.Fatalf("lapse plan: %v", err)
	}
	assertPosture("plan lapsed, intent kept", RetentionPosture{ZDRIntent: true, Entitled: false})

	if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET plan = 'pro' WHERE id = $1`, orgID); err != nil {
		t.Fatalf("restore plan: %v", err)
	}
	for _, malformed := range []string{`{"zdr":"true"}`, `{"zdr":1}`, `{"zdr":{"value":true}}`, `{"zdr":null}`, `{}`} {
		if _, err := db.Pool.Exec(ctx, `
UPDATE organizations
SET metadata = jsonb_set(COALESCE(metadata, '{}'::jsonb), '{interactiveRetention}', $2::jsonb, true)
WHERE id = $1`, orgID, malformed); err != nil {
			t.Fatalf("write malformed intent %s: %v", malformed, err)
		}
		assertPosture("malformed intent "+malformed, RetentionPosture{ZDRIntent: false, Entitled: true})
	}

	if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET deleted_at = NOW() WHERE id = $1`, orgID); err != nil {
		t.Fatalf("soft delete: %v", err)
	}
	if _, err := service.GetRetentionPosture(ctx, orgID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("soft-deleted org error=%v want ErrNotFound", err)
	}
	if _, err := service.GetRetentionPosture(ctx, "org-retention-posture-missing"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("unknown org error=%v want ErrNotFound", err)
	}
}
