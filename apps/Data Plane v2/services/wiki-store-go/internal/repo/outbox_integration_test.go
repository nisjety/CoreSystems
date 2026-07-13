package repo

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/model"
)

func TestWikiPageCommitAndEventIntentAreAtomic(t *testing.T) {
	databaseURL := os.Getenv("WIKI_TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("set WIKI_TEST_DATABASE_URL to a disposable local *_test database")
	}
	config, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatal(err)
	}
	if config.ConnConfig.Host != "localhost" && config.ConnConfig.Host != "127.0.0.1" && config.ConnConfig.Host != "::1" {
		t.Fatalf("refusing non-local test database host %q", config.ConnConfig.Host)
	}
	if !strings.HasSuffix(config.ConnConfig.Database, "_test") {
		t.Fatalf("refusing non-test database %q", config.ConnConfig.Database)
	}

	ctx := context.Background()
	admin, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		t.Fatal(err)
	}
	defer admin.Close()
	schema := "wiki_outbox_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _, _ = admin.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE") })

	isolatedConfig, _ := pgxpool.ParseConfig(databaseURL)
	isolatedConfig.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, isolatedConfig)
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	if _, err := pool.Exec(ctx, legacyWikiSchema); err != nil {
		t.Fatalf("create wiki schema: %v", err)
	}
	migrationPath := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", wikiOutboxMigration)
	migration, err := os.ReadFile(migrationPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := pool.Exec(ctx, string(migration)); err != nil {
		t.Fatalf("apply wiki outbox migration: %v", err)
	}

	repository := NewWikiRepo(pool)
	page, version, err := repository.CreatePage(ctx, model.CreatePageInput{
		OrgID: "org-success", WorkspaceID: "workspace-success", Title: "Title",
		Path: "/title", InitialContent: "content",
	})
	if err != nil || page == nil || version == nil {
		t.Fatalf("create page with outbox: page=%v version=%v err=%v", page, version, err)
	}
	var outboxOrg, payloadOrg, payloadVersion string
	if err := pool.QueryRow(ctx, `
		SELECT org_id, payload ->> 'org_id', payload ->> 'version_id'
		FROM wiki_event_outbox WHERE idempotency_key = $1
	`, "wiki.published:"+version.VersionID).Scan(&outboxOrg, &payloadOrg, &payloadVersion); err != nil {
		t.Fatalf("read atomic outbox intent: %v", err)
	}
	if outboxOrg != "org-success" || payloadOrg != outboxOrg || payloadVersion != version.VersionID {
		t.Fatalf("outbox identity mismatch: %q %q %q", outboxOrg, payloadOrg, payloadVersion)
	}

	if _, err := pool.Exec(ctx, `ALTER TABLE wiki_event_outbox
		ADD CONSTRAINT reject_rollback_probe CHECK (org_id <> 'org-rollback')`); err != nil {
		t.Fatal(err)
	}
	if _, _, err := repository.CreatePage(ctx, model.CreatePageInput{
		OrgID: "org-rollback", WorkspaceID: "workspace-rollback", Title: "Rollback",
		Path: "/rollback", InitialContent: "must not commit",
	}); err == nil {
		t.Fatal("page commit succeeded when its event intent failed")
	}
	var pageCount, versionCount int
	if err := pool.QueryRow(ctx, `SELECT
		(SELECT COUNT(*) FROM wiki_pages WHERE org_id = 'org-rollback'),
		(SELECT COUNT(*) FROM wiki_page_versions WHERE page_id IN
			(SELECT page_id FROM wiki_pages WHERE org_id = 'org-rollback'))
	`).Scan(&pageCount, &versionCount); err != nil {
		t.Fatal(err)
	}
	if pageCount != 0 || versionCount != 0 {
		t.Fatalf("partial wiki commit escaped rollback: pages=%d versions=%d", pageCount, versionCount)
	}
}
