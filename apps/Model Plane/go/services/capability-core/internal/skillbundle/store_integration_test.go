//go:build integration

package skillbundle

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/testcontainers/testcontainers-go"
	tcpostgres "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"
)

func setupBundleDB(t *testing.T) *pgxpool.Pool {
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
				WithOccurrence(2).WithStartupTimeout(60*time.Second),
		),
	)
	if err != nil {
		t.Fatalf("start postgres: %v", err)
	}
	t.Cleanup(func() {
		cleanupCtx, cleanupCancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cleanupCancel()
		_ = container.Terminate(cleanupCtx)
	})
	dsn, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect postgres: %v", err)
	}
	t.Cleanup(pool.Close)
	for _, migration := range []string{"0001_models.up.sql", "0003_capabilities_registry.up.sql", "0021_skill_bundles.up.sql"} {
		sql, readErr := os.ReadFile(filepath.Join("..", "..", "migrations", migration))
		if readErr != nil {
			t.Fatalf("read migration %s: %v", migration, readErr)
		}
		if _, execErr := pool.Exec(ctx, string(sql)); execErr != nil {
			t.Fatalf("apply migration %s: %v", migration, execErr)
		}
	}
	return pool
}

func bundleOf(t *testing.T, body string) Bundle {
	t.Helper()
	bundle, err := Parse(buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "references/shared.txt", body: []byte(body)},
	))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	return bundle
}

func TestStoreRoundTripsReplacesAndDeletesAnOrgsBundle(t *testing.T) {
	ctx := context.Background()
	store := NewStore(setupBundleDB(t))
	first := bundleOf(t, "first body")

	put, err := store.Put(ctx, "org-a", "skill_1", "user-admin", first)
	if err != nil {
		t.Fatalf("Put: %v", err)
	}
	if put.Replaced || put.Summary.FileCount != 2 {
		t.Fatalf("first put = %+v, want a new bundle of two files", put)
	}

	manifest, err := store.Manifest(ctx, "org-a", "skill_1")
	if err != nil {
		t.Fatalf("Manifest: %v", err)
	}
	if manifest.ManifestSHA256 != first.ManifestSHA256 || len(manifest.Files) != 2 {
		t.Fatalf("manifest = %+v, want the uploaded digest and two files", manifest)
	}
	body, err := store.Blob(ctx, "org-a", "skill_1", first.Files[0].SHA256)
	if err != nil {
		t.Fatalf("Blob: %v", err)
	}
	if len(body) == 0 {
		t.Fatal("blob body is empty")
	}

	// Another org cannot read these bytes, even by knowing the digest.
	if _, err := store.Blob(ctx, "org-b", "skill_1", first.Files[0].SHA256); !errors.Is(err, ErrNotFound) {
		t.Fatalf("cross-org blob read = %v, want ErrNotFound", err)
	}
	// A digest the skill's bundle does not name is refused for that skill.
	if _, err := store.Blob(ctx, "org-a", "skill_other", first.Files[0].SHA256); !errors.Is(err, ErrNotFound) {
		t.Fatalf("blob under another skill = %v, want ErrNotFound", err)
	}

	second := bundleOf(t, "second body")
	put, err = store.Put(ctx, "org-a", "skill_1", "user-admin", second)
	if err != nil {
		t.Fatalf("replace Put: %v", err)
	}
	if !put.Replaced {
		t.Fatal("second put did not report a replacement")
	}
	// The first bundle's unique shared-file blob is gone: no bundle names it any more.
	if _, err := store.Blob(ctx, "org-a", "skill_1", first.Files[1].SHA256); !errors.Is(err, ErrNotFound) {
		t.Fatalf("replaced bundle's blob = %v, want ErrNotFound", err)
	}

	summaries, err := store.List(ctx, "org-a", nil)
	if err != nil || len(summaries) != 1 || summaries[0].ManifestSHA256 != second.ManifestSHA256 {
		t.Fatalf("List = %+v, %v; want the replacement only", summaries, err)
	}

	if err := store.Delete(ctx, "org-a", "skill_1", "user-admin"); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if _, err := store.Manifest(ctx, "org-a", "skill_1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("manifest after delete = %v, want ErrNotFound", err)
	}
	if err := store.Delete(ctx, "org-a", "skill_1", "user-admin"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("second delete = %v, want ErrNotFound", err)
	}
}

func TestStoreListIsScopedToTheOrgAndTheRequestedSkills(t *testing.T) {
	ctx := context.Background()
	store := NewStore(setupBundleDB(t))
	if _, err := store.Put(ctx, "org-a", "skill_1", "user-admin", bundleOf(t, "one")); err != nil {
		t.Fatalf("Put org-a skill_1: %v", err)
	}
	if _, err := store.Put(ctx, "org-a", "skill_2", "user-admin", bundleOf(t, "two")); err != nil {
		t.Fatalf("Put org-a skill_2: %v", err)
	}
	if _, err := store.Put(ctx, "org-b", "skill_1", "user-b", bundleOf(t, "three")); err != nil {
		t.Fatalf("Put org-b skill_1: %v", err)
	}
	all, err := store.List(ctx, "org-a", nil)
	if err != nil || len(all) != 2 {
		t.Fatalf("org-a list = %+v, %v; want its two bundles", all, err)
	}
	some, err := store.List(ctx, "org-a", []string{"skill_2"})
	if err != nil || len(some) != 1 || some[0].SkillID != "skill_2" {
		t.Fatalf("narrowed list = %+v, %v; want skill_2 only", some, err)
	}
}
