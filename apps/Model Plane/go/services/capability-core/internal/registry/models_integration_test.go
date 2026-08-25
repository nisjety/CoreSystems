//go:build integration

package registry

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"

	"github.com/triodelab/model-plane/services/capability-core/internal/domain"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

func setupModelsRegistry(t *testing.T) (*ModelsRegistry, *pgxpool.Pool) {
	t.Helper()

	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	container, err := tcpostgres.Run(ctx,
		"postgres:16-alpine",
		tcpostgres.WithDatabase("capabilities"),
		tcpostgres.WithUsername("test"),
		tcpostgres.WithPassword("test"),
		testcontainers.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").
				WithOccurrence(2).
				WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres container: %v", err)
	}
	t.Cleanup(func() {
		termCtx, termCancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer termCancel()
		_ = container.Terminate(termCtx)
	})

	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("get conn string: %v", err)
	}

	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("open pool: %v", err)
	}
	t.Cleanup(pool.Close)

	migrationsDir := filepath.Join("..", "..", "migrations")
	for _, name := range allMigrationFilenames() {
		path := filepath.Join(migrationsDir, name)
		sqlBytes, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read migration %s: %v", name, err)
		}
		if _, err := pool.Exec(ctx, string(sqlBytes)); err != nil {
			t.Fatalf("apply migration %s: %v", name, err)
		}
	}

	reg, err := NewModelsRegistry(pool)
	if err != nil {
		t.Fatalf("new models registry: %v", err)
	}
	return reg, pool
}

func TestNewModelsRegistry_NilPool(t *testing.T) {
	if _, err := NewModelsRegistry(nil); !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("expected ErrInvalidArgument, got %v", err)
	}
}

func TestModelsRegistry_GetByName_Seed(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	m, err := reg.GetByName(ctx, uuid.Nil, "openai", "gpt-4o")
	if err != nil {
		t.Fatalf("GetByName: %v", err)
	}
	if m.Version != "2024-08-06" {
		t.Fatalf("expected version 2024-08-06, got %q", m.Version)
	}
	if m.OrgID != uuid.Nil {
		t.Fatalf("expected nil org, got %v", m.OrgID)
	}
	if m.Provider != "openai" || m.Name != "gpt-4o" {
		t.Fatalf("unexpected provider/name: %s/%s", m.Provider, m.Name)
	}
}

func TestModelsRegistry_GetByCapabilityID_Happy(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	m, err := reg.GetByCapabilityID(ctx, "cap.model.openai.gpt-4o")
	if err != nil {
		t.Fatalf("GetByCapabilityID: %v", err)
	}
	if m.Provider != "openai" || m.Name != "gpt-4o" {
		t.Fatalf("unexpected: %s/%s", m.Provider, m.Name)
	}
}

func TestModelsRegistry_GetByCapabilityID_BadPrefix(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	_, err := reg.GetByCapabilityID(ctx, "bogus")
	if !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected ErrCapabilityNotFound, got %v", err)
	}
}

func TestModelsRegistry_GetByCapabilityID_Malformed(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	_, err := reg.GetByCapabilityID(ctx, "cap.model.foo")
	if !errors.Is(err, domain.ErrInvalidArgument) {
		t.Fatalf("expected ErrInvalidArgument, got %v", err)
	}
}

func TestModelsRegistry_List_SeedCount(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	list, err := reg.List(ctx, ModelsFilter{})
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(list) != 4 {
		// 0002 seeds five rows; 0013 soft-deletes the decorative
		// google/gemini-1.5-pro row (no serving adapter exists), leaving four.
		t.Fatalf("expected 4 live seeded rows, got %d", len(list))
	}
}

func TestModelsRegistry_ListAsCapabilities_Shape(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	caps, err := reg.ListAsCapabilities(ctx, ModelsFilter{})
	if err != nil {
		t.Fatalf("ListAsCapabilities: %v", err)
	}
	if len(caps) != 4 {
		// Five seeded minus the gemini row 0013 soft-deletes.
		t.Fatalf("expected 4 caps, got %d", len(caps))
	}
	for _, c := range caps {
		if c.Kind != models.KindModel {
			t.Fatalf("expected KindModel, got %v", c.Kind)
		}
		if !strings.HasPrefix(c.IdempotencyKey, "fbc1d94e94d756ede12c527b3d59e2204f58a623e6bd5a3d679eb03d93f22637:") {
			t.Fatalf("unexpected idempotency key: %q", c.IdempotencyKey)
		}
		if len(c.EnabledForScopes) != 1 || c.EnabledForScopes[0] != "global" {
			t.Fatalf("expected [global] scopes, got %v", c.EnabledForScopes)
		}
	}
}

func TestModelsRegistry_Upsert_InsertThenUpdate(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	m1 := &Model{
		Provider:    "acme",
		Name:        "test-model",
		Version:     "v1",
		Description: "first",
	}
	up1, err := reg.Upsert(ctx, m1)
	if err != nil {
		t.Fatalf("first Upsert: %v", err)
	}
	if up1.ID == uuid.Nil {
		t.Fatalf("expected non-nil id")
	}

	m2 := &Model{
		Provider:    "acme",
		Name:        "test-model",
		Version:     "v2",
		Description: "second",
	}
	up2, err := reg.Upsert(ctx, m2)
	if err != nil {
		t.Fatalf("second Upsert: %v", err)
	}
	if up2.ID != up1.ID {
		t.Fatalf("expected same id on conflict, got %v vs %v", up1.ID, up2.ID)
	}

	got, err := reg.Get(ctx, up1.ID)
	if err != nil {
		t.Fatalf("Get after upsert: %v", err)
	}
	if got.Version != "v2" {
		t.Fatalf("expected version v2 after update, got %q", got.Version)
	}
}

func TestModelsRegistry_Delete_SoftThenNotFound(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	m, err := reg.GetByName(ctx, uuid.Nil, "openai", "gpt-4o-mini")
	if err != nil {
		t.Fatalf("GetByName for delete: %v", err)
	}

	if err := reg.Delete(ctx, m.ID); err != nil {
		t.Fatalf("first Delete: %v", err)
	}
	if err := reg.Delete(ctx, m.ID); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected ErrCapabilityNotFound on second delete, got %v", err)
	}
}

// allMigrationFilenames lists every up migration in order. Every
// ModelsRegistry read path selects privacy_tier/residency (migration 0013),
// so tests against this registry must always run against the fully-migrated
// schema; applying a prefix of the migrations reproduces production's
// pre-upgrade state that this code no longer supports.
func allMigrationFilenames() []string {
	return []string{
		"0001_models.up.sql",
		"0002_seed_models.up.sql",
		"0003_capabilities_registry.up.sql",
		"0004_seed_self_owned_systems.up.sql",
		"0005_seed_operating_map_capability.up.sql",
		"0006_capability_availability_contract.up.sql",
		"0007_tenant_scopes_and_risk_constraints.up.sql",
		"0008_execution_dispatch_capabilities.up.sql",
		"0009_mcp_oauth_client_columns.up.sql",
		"0010_sandbox_code_execution_capability.up.sql",
		"0011_conversation_ticket_action_capability.up.sql",
		"0012_run_watch_subscriptions.up.sql",
		"0013_privacy_tier_columns.up.sql",
	}
}

func TestModelsRegistry_Migration0013_DefaultsToUnspecified(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	m, err := reg.GetByName(ctx, uuid.Nil, "openai", "gpt-4o")
	if err != nil {
		t.Fatalf("GetByName: %v", err)
	}
	if m.PrivacyTier != models.PrivacyTierUnspecified {
		t.Fatalf("expected default privacy_tier=unspecified, got %q", m.PrivacyTier)
	}
	if m.Residency != "" {
		t.Fatalf("expected default residency='', got %q", m.Residency)
	}
}

func TestModelsRegistry_Migration0013_RejectsUnknownPrivacyTier(t *testing.T) {
	_, pool := setupModelsRegistry(t)
	ctx := context.Background()

	_, err := pool.Exec(ctx, `UPDATE models SET privacy_tier = 'bogus' WHERE provider = 'openai' AND name = 'gpt-4o'`)
	if err == nil {
		t.Fatal("expected the models_privacy_tier_check constraint to reject an unknown tier label")
	}
}

func TestModelsRegistry_Migration0013_SoftDeletesDecorativeGeminiSeed(t *testing.T) {
	reg, _ := setupModelsRegistry(t)
	ctx := context.Background()

	if _, err := reg.GetByName(ctx, uuid.Nil, "google", "gemini-1.5-pro"); !errors.Is(err, domain.ErrCapabilityNotFound) {
		t.Fatalf("expected the decorative google/gemini-1.5-pro seed row to be soft-deleted, got err=%v", err)
	}

	list, err := reg.List(ctx, ModelsFilter{})
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	for _, m := range list {
		if m.Provider == "google" && m.Name == "gemini-1.5-pro" {
			t.Fatal("soft-deleted gemini row must not appear in List (deleted_at IS NULL filter)")
		}
	}
}
