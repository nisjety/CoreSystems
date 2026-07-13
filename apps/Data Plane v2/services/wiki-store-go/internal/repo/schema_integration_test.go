package repo

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/model"
)

const wikiSchemaMigration = "20260710090000_reconcile_wiki_runtime_schema.sql"

func TestWikiSchemaMigrationCarriesRuntimeContract(t *testing.T) {
	sql := readWikiSchemaMigration(t)
	required := []string{
		"ALTER TABLE wiki_pages ADD COLUMN IF NOT EXISTS deleted_at",
		"LOWER(COALESCE(page_status, '')) = 'deleted'",
		"ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS org_id",
		"ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS source_type",
		"ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS source_ref",
		"ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS sync_status",
		"ALTER TABLE wiki_source_logs ADD COLUMN IF NOT EXISTS details",
		"ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS org_id",
		"ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS action",
		"ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS actor",
		"ALTER TABLE wiki_maintenance_logs ADD COLUMN IF NOT EXISTS details",
		"sync_wiki_source_log_contract_before_write",
		"sync_wiki_maintenance_log_contract_before_write",
		"IS DISTINCT FROM page.org_id",
	}
	for _, fragment := range required {
		if !strings.Contains(sql, fragment) {
			t.Errorf("migration missing %q", fragment)
		}
	}
	if strings.Contains(strings.ToUpper(sql), "DROP COLUMN") || strings.Contains(strings.ToUpper(sql), "DROP TABLE") {
		t.Fatal("wiki reconciliation migration must be forward-only and additive")
	}
}

func TestPostgresInitCarriesCurrentAndLegacyWikiColumns(t *testing.T) {
	path := filepath.Join("..", "..", "..", "..", "infra", "postgres", "init.sql")
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read init.sql: %v", err)
	}
	sql := string(contents)
	required := []string{
		"deleted_at         TIMESTAMPTZ",
		"source_type         TEXT         NOT NULL",
		"synthesis_prompt_hash TEXT",
		"action       TEXT         NOT NULL",
		"issue_type   TEXT",
	}
	for _, fragment := range required {
		if !strings.Contains(sql, fragment) {
			t.Errorf("init.sql missing wiki contract %q", fragment)
		}
	}
}

func TestWikiSchemaMigrationSupportsLegacyRowsAndRepository(t *testing.T) {
	databaseURL := os.Getenv("WIKI_TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("set WIKI_TEST_DATABASE_URL to a disposable local *_test database")
	}

	adminConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse WIKI_TEST_DATABASE_URL: %v", err)
	}
	host := adminConfig.ConnConfig.Host
	if host != "localhost" && host != "127.0.0.1" && host != "::1" {
		t.Fatalf("refusing non-local test database host %q", host)
	}
	if !strings.HasSuffix(adminConfig.ConnConfig.Database, "_test") {
		t.Fatalf("refusing database %q: name must end in _test", adminConfig.ConnConfig.Database)
	}

	ctx := context.Background()
	adminPool, err := pgxpool.NewWithConfig(ctx, adminConfig)
	if err != nil {
		t.Fatalf("connect admin pool: %v", err)
	}
	defer adminPool.Close()

	schema := "wiki_schema_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	if _, err := adminPool.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create isolated schema: %v", err)
	}
	t.Cleanup(func() {
		_, _ = adminPool.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE")
	})

	testConfig, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse isolated pool config: %v", err)
	}
	testConfig.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, testConfig)
	if err != nil {
		t.Fatalf("connect isolated pool: %v", err)
	}
	defer pool.Close()

	if _, err := pool.Exec(ctx, legacyWikiSchema); err != nil {
		t.Fatalf("create legacy schema: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_pages (page_id, org_id, workspace_id, title, path) VALUES ('page-1', 'org-1', 'workspace-1', 'Title', '/title');
		INSERT INTO wiki_pages (page_id, org_id, workspace_id, title, path, page_status) VALUES ('page-deleted', 'org-1', 'workspace-1', 'Deleted', '/deleted', 'deleted');
		INSERT INTO wiki_pages (page_id, org_id, workspace_id, title, path) VALUES ('page-2', 'org-2', 'workspace-2', 'Other', '/other');
		INSERT INTO wiki_page_versions (version_id, page_id, content, version_status) VALUES ('version-2', 'page-2', 'other tenant', 'published');
		INSERT INTO wiki_source_logs (log_id, page_id, synthesis_prompt_hash, metadata) VALUES ('source-legacy', 'page-1', 'legacy-ref', '{"legacy":true}');
		INSERT INTO wiki_maintenance_logs (log_id, page_id, issue_type, issue_details, metadata) VALUES ('maintenance-legacy', 'page-1', 'stale', '{"legacy":true}', '{}');
	`); err != nil {
		t.Fatalf("seed legacy rows: %v", err)
	}
	if _, err := pool.Exec(ctx, readWikiSchemaMigration(t)); err != nil {
		t.Fatalf("apply wiki migration: %v", err)
	}
	if _, err := pool.Exec(ctx, readWikiSchemaMigration(t)); err != nil {
		t.Fatalf("reapply idempotent wiki migration: %v", err)
	}

	wikiRepo := NewWikiRepo(pool)
	pages, total, err := wikiRepo.ListPages(ctx, "org-1", "", "", 20, 0)
	if err != nil || total != 1 || len(pages) != 1 {
		t.Fatalf("ListPages after migration = (%d, %d, %v)", len(pages), total, err)
	}
	var deletedAtSet bool
	if err := pool.QueryRow(ctx, "SELECT deleted_at IS NOT NULL FROM wiki_pages WHERE page_id = 'page-deleted'").Scan(&deletedAtSet); err != nil || !deletedAtSet {
		t.Fatalf("legacy deleted page backfill = (%v, %v)", deletedAtSet, err)
	}
	sourceLogs, sourceTotal, err := wikiRepo.ListSourceLogs(ctx, "org-1", "page-1", 20, 0)
	if err != nil || sourceTotal != 1 || len(sourceLogs) != 1 || sourceLogs[0].SourceRef != "legacy-ref" {
		t.Fatalf("ListSourceLogs legacy compatibility = (%+v, %d, %v)", sourceLogs, sourceTotal, err)
	}
	maintenanceLogs, maintenanceTotal, err := wikiRepo.ListMaintenanceLogs(ctx, "org-1", "page-1", 20, 0)
	if err != nil || maintenanceTotal != 1 || len(maintenanceLogs) != 1 || maintenanceLogs[0].Action != "stale" {
		t.Fatalf("ListMaintenanceLogs legacy compatibility = (%+v, %d, %v)", maintenanceLogs, maintenanceTotal, err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_source_logs (log_id, page_id, synthesis_prompt_hash, metadata)
		VALUES ('source-old-writer', 'page-1', 'old-writer-ref', '{"old_writer":true}');
		INSERT INTO wiki_maintenance_logs (log_id, page_id, issue_type, issue_details)
		VALUES ('maintenance-old-writer', 'page-1', 'orphan', '{"old_writer":true}');
	`); err != nil {
		t.Fatalf("legacy writers after migration: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO wiki_source_logs (log_id, org_id, page_id, source_type, source_ref, sync_status, details)
		VALUES ('source-mismatched-org', 'org-2', 'page-1', 'document', 'mismatch', 'synced', '{}');
	`); err != nil {
		t.Fatalf("insert tenant-mismatch probe: %v", err)
	}
	var oldSourceOrg, oldSourceRef, oldMaintenanceOrg, oldMaintenanceAction string
	if err := pool.QueryRow(ctx, "SELECT org_id, source_ref FROM wiki_source_logs WHERE log_id = 'source-old-writer'").Scan(&oldSourceOrg, &oldSourceRef); err != nil {
		t.Fatalf("read legacy source writer row: %v", err)
	}
	if err := pool.QueryRow(ctx, "SELECT org_id, action FROM wiki_maintenance_logs WHERE log_id = 'maintenance-old-writer'").Scan(&oldMaintenanceOrg, &oldMaintenanceAction); err != nil {
		t.Fatalf("read legacy maintenance writer row: %v", err)
	}
	if oldSourceOrg != "org-1" || oldSourceRef != "old-writer-ref" || oldMaintenanceOrg != "org-1" || oldMaintenanceAction != "orphan" {
		t.Fatalf("legacy writer compatibility = (%q, %q, %q, %q)", oldSourceOrg, oldSourceRef, oldMaintenanceOrg, oldMaintenanceAction)
	}
	var correctedOrg string
	if err := pool.QueryRow(ctx, "SELECT org_id FROM wiki_source_logs WHERE log_id = 'source-mismatched-org'").Scan(&correctedOrg); err != nil || correctedOrg != "org-1" {
		t.Fatalf("tenant mismatch correction = (%q, %v)", correctedOrg, err)
	}

	createdSource, err := wikiRepo.CreateSourceLog(ctx, model.CreateSourceLogInput{
		OrgID: "org-1", PageID: "page-1", SourceType: "document", SourceRef: "source-new", SyncStatus: "synced", Details: json.RawMessage(`{"new":true}`),
	})
	if err != nil || createdSource.OrgID != "org-1" {
		t.Fatalf("CreateSourceLog after migration = (%+v, %v)", createdSource, err)
	}
	createdMaintenance, err := wikiRepo.CreateMaintenanceLog(ctx, model.CreateMaintenanceLogInput{
		OrgID: "org-1", PageID: "page-1", Action: "orphan", Actor: "test", Details: json.RawMessage(`{"new":true}`),
	})
	if err != nil || createdMaintenance.OrgID != "org-1" {
		t.Fatalf("CreateMaintenanceLog after migration = (%+v, %v)", createdMaintenance, err)
	}

	t.Run("repository rejects cross-tenant page references", func(t *testing.T) {
		if _, err := wikiRepo.GetVersionForPage(ctx, "org-1", "page-2", "version-2"); err == nil {
			t.Fatal("GetVersionForPage exposed another tenant's version")
		}
		if _, err := wikiRepo.CreateVersion(ctx, model.UpdateVersionInput{OrgID: "org-1", PageID: "page-2", NewContent: "blocked"}); err == nil {
			t.Fatal("CreateVersion accepted another tenant's page")
		}
		if _, err := wikiRepo.SubmitProposal(ctx, model.SubmitProposalInput{OrgID: "org-1", PageID: "page-2", ProposedContent: "blocked"}); err == nil {
			t.Fatal("SubmitProposal accepted another tenant's page")
		}
		if _, err := wikiRepo.CreateSourceLog(ctx, model.CreateSourceLogInput{OrgID: "org-1", PageID: "page-2", SourceType: "document", SourceRef: "blocked", SyncStatus: "synced"}); err == nil {
			t.Fatal("CreateSourceLog accepted another tenant's page")
		}
		if _, err := wikiRepo.CreateMaintenanceLog(ctx, model.CreateMaintenanceLogInput{OrgID: "org-1", PageID: "page-2", Action: "blocked", Actor: "test"}); err == nil {
			t.Fatal("CreateMaintenanceLog accepted another tenant's page")
		}
	})
}

func readWikiSchemaMigration(t *testing.T) string {
	t.Helper()
	path := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", wikiSchemaMigration)
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration %s: %v", path, err)
	}
	return string(contents)
}

const legacyWikiSchema = `
	CREATE TABLE wiki_pages (
		page_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, workspace_id TEXT NOT NULL,
		title TEXT NOT NULL, path TEXT NOT NULL, current_version_id TEXT,
		page_status TEXT DEFAULT 'draft', backlinks JSONB, metadata JSONB,
		created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
	);
	CREATE TABLE wiki_source_logs (
		log_id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
		original_chunks JSONB, processing_model TEXT, synthesis_prompt_hash TEXT,
		metadata JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
	);
	CREATE TABLE wiki_page_versions (
		version_id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
		content TEXT, source_refs JSONB, proposed_by_agent TEXT, proposed_by_user TEXT,
		approved_by TEXT, edit_reason TEXT, version_status TEXT DEFAULT 'draft', metadata JSONB,
		created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), published_at TIMESTAMPTZ
	);
	CREATE TABLE wiki_proposals (
		proposal_id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
		org_id TEXT NOT NULL, proposed_content TEXT NOT NULL, edit_reason TEXT,
		proposed_by_agent TEXT, source_refs JSONB, proposal_status TEXT DEFAULT 'pending',
		reviewed_by TEXT, metadata JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
	);
	CREATE TABLE wiki_maintenance_logs (
		log_id TEXT PRIMARY KEY, page_id TEXT NOT NULL REFERENCES wiki_pages(page_id) ON DELETE CASCADE,
		issue_type TEXT, issue_details JSONB, proposed_fix TEXT, issue_status TEXT DEFAULT 'open',
		metadata JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), resolved_at TIMESTAMPTZ
	);
`
