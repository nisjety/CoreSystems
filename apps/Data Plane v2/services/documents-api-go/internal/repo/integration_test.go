//go:build integration

package repo_test

// Integration tests for DocumentRepo using testcontainers-postgres.
// Run with: go test -tags integration -race ./...
//
// Requires Docker available locally. Skipped on CI runs without Docker.

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
	tc "github.com/testcontainers/testcontainers-go"
	pgmod "github.com/testcontainers/testcontainers-go/modules/postgres"
	"github.com/testcontainers/testcontainers-go/wait"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
)

// schemaSQL is a slim subset of init.sql sufficient to test the document path.
const schemaSQL = `
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

CREATE TABLE IF NOT EXISTS documents (
    document_id  TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id       TEXT NOT NULL,
    source       TEXT NOT NULL DEFAULT '',
    type         TEXT NOT NULL DEFAULT '',
    title        TEXT NOT NULL DEFAULT '',
    content      TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'pending',
    metadata     JSONB NOT NULL DEFAULT '{}',
    error_message TEXT,
    zdr_classification TEXT NOT NULL DEFAULT 'internal',
    zdr_reason   TEXT,
    extraction_trace JSONB,
    created_by   TEXT,
    deleted_by   TEXT,
    owner_id     TEXT NOT NULL DEFAULT 'org-system-account',
    visibility   TEXT NOT NULL DEFAULT 'org',
    idempotency_key TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at   TIMESTAMPTZ,
    CONSTRAINT documents_visibility_chk CHECK (visibility IN ('private', 'org', 'shared'))
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_idempotency
    ON documents (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL AND deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS documents_outbox (
    outbox_id BIGSERIAL PRIMARY KEY,
    org_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    published BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS org_versions (
    org_id TEXT PRIMARY KEY,
    version BIGINT NOT NULL,
    bumped_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
`

func setupPostgres(t *testing.T) (*pgxpool.Pool, func()) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()

	container, err := pgmod.Run(ctx,
		"postgres:16-alpine",
		pgmod.WithDatabase("dataplane_test"),
		pgmod.WithUsername("test"),
		pgmod.WithPassword("test"),
		tc.WithWaitStrategy(
			wait.ForLog("database system is ready to accept connections").WithOccurrence(2).WithStartupTimeout(30*time.Second),
		),
	)
	if err != nil {
		t.Skipf("docker not available, skipping testcontainers: %v", err)
	}

	connStr, err := container.ConnectionString(ctx, "sslmode=disable")
	if err != nil {
		t.Fatalf("connection string: %v", err)
	}

	// Apply schema using database/sql for one-shot DDL (avoids pgx pool warmup).
	db, err := sql.Open("pgx", connStr)
	if err == nil {
		// pgx stdlib not registered without import; fall back to pgxpool directly.
		_ = db.Close()
	}

	pool, err := pgxpool.New(ctx, connStr)
	if err != nil {
		t.Fatalf("pgx pool: %v", err)
	}

	if _, err := pool.Exec(ctx, schemaSQL); err != nil {
		t.Fatalf("apply schema: %v", err)
	}

	cleanup := func() {
		pool.Close()
		_ = container.Terminate(context.Background())
	}
	return pool, cleanup
}

// ─── Tests ──────────────────────────────────────────────────────────────────

func TestCreateAndGet(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()

	result, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID:   "org-test",
		Source:  "src",
		Type:    "article",
		Title:   "Hello",
		Content: "World",
	})
	if err != nil {
		t.Fatalf("create: %v", err)
	}
	if result.Reused {
		t.Errorf("first create should not be reused")
	}
	if result.Document.DocumentID == "" {
		t.Errorf("expected document_id")
	}

	got, err := r.Get(ctx, "org-test", result.Document.DocumentID, "", nil)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if got.Title != "Hello" {
		t.Errorf("expected Hello, got %q", got.Title)
	}
}

// TestZDRClassificationRoundTrip proves the GDPR/ZDR wire fix end-to-end at the
// persistence boundary: a non-default classification (as Quarry-v2's ingest
// client now sends after mapping PrivacyClassification → zdr_classification) is
// stored verbatim and read back — NOT silently coerced to the "internal"
// default. This is the receiving half of the fix that makes retrieval-engine's
// reject-mode filter (WHERE zdr_classification = 'restricted') operate on real
// data. Requires docker (testcontainers); build with -tags integration.
func TestZDRClassificationRoundTrip(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()

	// A restricted document must persist as "restricted" so retrieval filters it.
	restricted, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID:             "org-zdr",
		Source:            "quarry",
		Type:              "web_page",
		Title:             "Secret",
		Content:           "confidential body",
		ZDRClassification: "restricted",
	})
	if err != nil {
		t.Fatalf("create restricted: %v", err)
	}
	gotRestricted, err := r.Get(ctx, "org-zdr", restricted.Document.DocumentID, "", nil)
	if err != nil {
		t.Fatalf("get restricted: %v", err)
	}
	if gotRestricted.ZDRClassification != "restricted" {
		t.Fatalf("expected zdr_classification=restricted to round-trip, got %q", gotRestricted.ZDRClassification)
	}

	// An empty classification (no policy computed) still defaults to "internal".
	internal, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID:   "org-zdr",
		Source:  "quarry",
		Type:    "web_page",
		Title:   "Ordinary",
		Content: "ordinary body",
	})
	if err != nil {
		t.Fatalf("create default: %v", err)
	}
	gotInternal, err := r.Get(ctx, "org-zdr", internal.Document.DocumentID, "", nil)
	if err != nil {
		t.Fatalf("get default: %v", err)
	}
	if gotInternal.ZDRClassification != "internal" {
		t.Fatalf("expected empty zdr_classification to default to internal, got %q", gotInternal.ZDRClassification)
	}
}

func TestIdempotency(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()

	input := model.CreateDocumentInput{
		OrgID:          "org-idem",
		Source:         "s",
		Type:           "t",
		Title:          "First",
		Content:        "C1",
		IdempotencyKey: "key-123",
	}

	first, err := r.Create(ctx, input)
	if err != nil {
		t.Fatalf("first create: %v", err)
	}
	if first.Reused {
		t.Errorf("first create should not be reused")
	}

	// Same key again — must return existing doc, not create new one
	input.Title = "Second"
	input.Content = "C2"
	second, err := r.Create(ctx, input)
	if err != nil {
		t.Fatalf("second create: %v", err)
	}
	if !second.Reused {
		t.Errorf("second create should be reused")
	}
	if second.Document.DocumentID != first.Document.DocumentID {
		t.Errorf("idempotent create returned different doc_id")
	}
	if second.Document.Title != "First" {
		t.Errorf("expected original title, got %q", second.Document.Title)
	}

	// Different org, same key — must create separately
	input.OrgID = "org-other"
	other, err := r.Create(ctx, input)
	if err != nil {
		t.Fatalf("cross-org create: %v", err)
	}
	if other.Reused {
		t.Errorf("cross-org should not collide on idempotency key")
	}
	if other.Document.DocumentID == first.Document.DocumentID {
		t.Errorf("cross-org returned same doc_id — TENANT LEAK")
	}
}

func TestSoftDeleteOrgScope(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()

	doc, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID: "org-A", Source: "s", Type: "t", Title: "A", Content: "x",
	})
	if err != nil {
		t.Fatalf("create: %v", err)
	}

	// Org B tries to delete org A's doc — must fail
	err = r.SoftDelete(ctx, "org-B", doc.Document.DocumentID, "")
	if err == nil {
		t.Errorf("cross-org delete must fail")
	}

	// Doc must still be retrievable from org A
	got, err := r.Get(ctx, "org-A", doc.Document.DocumentID, "", nil)
	if err != nil {
		t.Fatalf("doc disappeared: %v", err)
	}
	if got.DocumentID == "" {
		t.Errorf("doc unexpectedly deleted")
	}
}

func TestCreateUpdateDeleteLifecycleOutboxIsAtomic(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()
	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()
	factory := func(document *model.Document, updated bool) (string, []byte, error) {
		eventType := "dataplane.documents.created"
		if updated {
			eventType = "dataplane.documents.updated"
		}
		payload, err := json.Marshal(map[string]any{
			"document_id": document.DocumentID,
			"org_id":      document.OrgID,
			"zdr":         false,
		})
		return eventType, payload, err
	}
	input := model.CreateDocumentInput{
		OrgID: "org-outbox", Source: "s", Type: "t", Title: "A", Content: "one",
		OwnerID: "user-a", CreatedBy: "user-a", IdempotencyKey: "outbox-fixture-1",
	}
	created, err := r.CreateWithOutbox(ctx, input, factory)
	if err != nil {
		t.Fatalf("create with outbox: %v", err)
	}
	var count int
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM documents_outbox").Scan(&count); err != nil || count != 1 {
		t.Fatalf("created outbox count=%d err=%v", count, err)
	}
	reused, err := r.CreateWithOutbox(ctx, input, factory)
	if err != nil || !reused.Reused {
		t.Fatalf("idempotent reuse: result=%+v err=%v", reused, err)
	}
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM documents_outbox").Scan(&count); err != nil || count != 1 {
		t.Fatalf("reuse emitted duplicate outbox count=%d err=%v", count, err)
	}
	input.Content = "two"
	updated, err := r.CreateWithOutbox(ctx, input, factory)
	if err != nil || !updated.Updated {
		t.Fatalf("update with outbox: result=%+v err=%v", updated, err)
	}
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM documents_outbox").Scan(&count); err != nil || count != 2 {
		t.Fatalf("updated outbox count=%d err=%v", count, err)
	}
	deletePayload := []byte(fmt.Sprintf(`{"document_id":%q,"org_id":"org-outbox","zdr":false}`, created.Document.DocumentID))
	if err := r.SoftDeleteWithOutbox(ctx, "org-outbox", created.Document.DocumentID, "user-a", "dataplane.documents.deleted", deletePayload); err != nil {
		t.Fatalf("delete with outbox: %v", err)
	}
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM documents_outbox").Scan(&count); err != nil || count != 3 {
		t.Fatalf("deleted outbox count=%d err=%v", count, err)
	}

	if _, err := pool.Exec(ctx, "DROP TABLE documents_outbox"); err != nil {
		t.Fatalf("drop outbox for rollback proof: %v", err)
	}
	input.IdempotencyKey = "outbox-fixture-2"
	input.Content = "rollback"
	if _, err := r.CreateWithOutbox(ctx, input, factory); err == nil {
		t.Fatal("create succeeded without durable outbox")
	}
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM documents WHERE idempotency_key='outbox-fixture-2'").Scan(&count); err != nil || count != 0 {
		t.Fatalf("document commit escaped failed outbox count=%d err=%v", count, err)
	}
}

func TestListOrgScoped(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()

	for _, org := range []string{"a", "b", "c"} {
		for i := 0; i < 3; i++ {
			meta, _ := json.Marshal(map[string]int{"i": i})
			if _, err := r.Create(ctx, model.CreateDocumentInput{
				OrgID:    "org-" + org,
				Source:   "s",
				Type:     "t",
				Title:    fmt.Sprintf("%s-%d", org, i),
				Content:  "x",
				Metadata: meta,
			}); err != nil {
				t.Fatalf("create: %v", err)
			}
		}
	}

	res, err := r.List(ctx, model.ListDocumentsInput{OrgID: "org-b", Limit: 100})
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if res.Total != 3 {
		t.Errorf("expected 3 docs for org-b, got %d", res.Total)
	}
	for _, d := range res.Documents {
		if d.OrgID != "org-b" {
			t.Errorf("got doc from %q in org-b list", d.OrgID)
		}
	}
}
