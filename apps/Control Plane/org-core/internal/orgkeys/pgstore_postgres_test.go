package orgkeys

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// The Postgres tests run against the disposable lifecycle database the other
// org-core Postgres tests use and are skipped without
// CONTROL_LIFECYCLE_TEST_DATABASE_URL (org-service.yml).

func orgKeysPostgres(t *testing.T) (context.Context, *database.DB, *Service) {
	t.Helper()
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	t.Cleanup(cancel)
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	t.Cleanup(db.Close)
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}
	repo := orgcore.NewRepository(db)
	for _, orgID := range []string{"org-keys-a", "org-keys-b"} {
		if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET deleted_at = NULL WHERE id = $1`, orgID); err != nil {
			t.Fatalf("restore %s: %v", orgID, err)
		}
		if err := repo.ProvisionOrganizationWithOwner(ctx, orgcore.Organization{ID: orgID, Name: orgID, Plan: "enterprise", Status: "active"}, "owner-"+orgID); err != nil {
			t.Fatalf("seed %s: %v", orgID, err)
		}
	}
	for _, statement := range []string{
		`DELETE FROM org_key_encryption_keys WHERE org_id LIKE 'org-keys-%'`,
		`DELETE FROM org_key_audit_outbox WHERE org_id LIKE 'org-keys-%'`,
		`DELETE FROM org_key_master_canaries`,
	} {
		if _, err := db.Pool.Exec(ctx, statement); err != nil {
			t.Fatalf("reset org key rows: %v", err)
		}
	}
	return ctx, db, NewService(NewPostgresStore(db), testMaster(t, "v1", map[string]byte{"v1": 0x11}))
}

func TestPostgresRoundtripRotateAndDestroy(t *testing.T) {
	ctx, db, service := orgKeysPostgres(t)
	key, err := service.GenerateDataKey(ctx, "org-keys-a", "quarry-edge", PurposeQuarryProfile, profileA)
	if err != nil {
		t.Fatalf("generate: %v", err)
	}
	dek, _, err := service.Unwrap(ctx, "org-keys-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK)
	if err != nil || !bytes.Equal(dek, key.DEK) {
		t.Fatalf("unwrap: err=%v", err)
	}
	if _, err := service.Rotate(ctx, "org-keys-a", "operator"); err != nil {
		t.Fatalf("rotate: %v", err)
	}
	rewrapped, err := service.Rewrap(ctx, "org-keys-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK)
	if err != nil || rewrapped.KeyID != "oc:2" {
		t.Fatalf("rewrap: %+v err=%v", rewrapped, err)
	}
	// Row-level security: org-b's scope cannot open org-a's blob.
	if _, err := service.GenerateDataKey(ctx, "org-keys-b", "quarry-edge", PurposeQuarryProfile, profileA); err != nil {
		t.Fatalf("generate org-b: %v", err)
	}
	if _, _, err := service.Unwrap(ctx, "org-keys-b", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK); !errors.Is(err, ErrInvalidContext) {
		t.Fatalf("foreign org unwrap: err=%v, want ErrInvalidContext", err)
	}
	if count, err := service.Destroy(ctx, "org-keys-a", "operator"); err != nil || count != 2 {
		t.Fatalf("destroy: count=%d err=%v", count, err)
	}
	if _, _, err := service.Unwrap(ctx, "org-keys-a", "quarry-edge", PurposeQuarryProfile, profileA, key.KeyID, key.WrappedDEK); !errors.Is(err, ErrDestroyed) {
		t.Fatalf("unwrap after destroy: err=%v, want ErrDestroyed", err)
	}
	var material int
	if err := db.Pool.QueryRow(ctx, `SELECT count(*) FROM org_key_encryption_keys WHERE org_id = 'org-keys-a' AND wrapped_kek IS NOT NULL`).Scan(&material); err != nil || material != 0 {
		t.Fatalf("key material left after destroy: %d err=%v", material, err)
	}
	var audits int
	if err := db.Pool.QueryRow(ctx, `SELECT count(*) FROM org_key_audit_outbox WHERE org_id = 'org-keys-a'`).Scan(&audits); err != nil || audits < 3 {
		t.Fatalf("audit intents for org-a = %d err=%v, want created, rotated and destroyed", audits, err)
	}
}

func TestPostgresHardDeleteCascadesKeyRows(t *testing.T) {
	ctx, db, service := orgKeysPostgres(t)
	if _, err := service.GenerateDataKey(ctx, "org-keys-b", "quarry-edge", PurposeQuarryProfile, profileA); err != nil {
		t.Fatalf("generate: %v", err)
	}
	if _, err := db.Pool.Exec(ctx, `SELECT gdpr_hard_delete_organization($1)`, "org-keys-b"); err != nil {
		t.Fatalf("hard delete: %v", err)
	}
	var rows int
	if err := db.Pool.QueryRow(ctx, `SELECT count(*) FROM org_key_encryption_keys WHERE org_id = 'org-keys-b'`).Scan(&rows); err != nil || rows != 0 {
		t.Fatalf("key rows after hard delete = %d err=%v, want 0 (FK cascade)", rows, err)
	}
}

func TestPostgresMigrationIsIdempotent(t *testing.T) {
	ctx, db, _ := orgKeysPostgres(t)
	body, err := os.ReadFile(filepath.Join("..", "..", "migrations", migrationUp))
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	tx, err := db.Pool.Begin(ctx)
	if err != nil {
		t.Fatalf("begin: %v", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	// Already applied by RunMigrations; twice more must change nothing and fail nothing.
	for i := 0; i < 2; i++ {
		if _, err := tx.Exec(ctx, string(body)); err != nil {
			t.Fatalf("re-apply %d: %v", i+1, err)
		}
	}
}

func TestPostgresCanaryRejectsWrongMasterKey(t *testing.T) {
	ctx, db, service := orgKeysPostgres(t)
	if err := service.Ready(ctx); err != nil {
		t.Fatalf("ready: %v", err)
	}
	wrong := NewService(NewPostgresStore(db), testMaster(t, "v1", map[string]byte{"v1": 0x44}))
	if _, err := wrong.GenerateDataKey(ctx, "org-keys-a", "quarry-edge", PurposeQuarryProfile, profileA); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("wrong master key: err=%v, want ErrUnavailable", err)
	}
}
