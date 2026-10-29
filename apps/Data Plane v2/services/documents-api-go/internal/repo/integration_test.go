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
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
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
    document_date TIMESTAMPTZ,
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

CREATE TABLE IF NOT EXISTS source_objects (
    source_object_id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
    org_id TEXT NOT NULL,
    connector TEXT NOT NULL,
    source TEXT NOT NULL,
    external_id TEXT NOT NULL,
    site_id TEXT,
    drive_id TEXT,
    item_id TEXT,
    parent_id TEXT,
    path TEXT,
    name TEXT NOT NULL,
    mime_type TEXT,
    size_bytes BIGINT,
    etag TEXT,
    ctag TEXT,
    quickxor_hash TEXT,
    sha1_hash TEXT,
    content_hash TEXT,
    acl_tags TEXT[] NOT NULL DEFAULT '{}',
    metadata JSONB NOT NULL DEFAULT '{}',
    modified_at TIMESTAMPTZ,
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, connector, external_id)
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
	grantScopedRuntimeRole(ctx, t, pool)

	cleanup := func() {
		pool.Close()
		_ = container.Terminate(context.Background())
	}
	return pool, cleanup
}

// grantScopedRuntimeRole makes `dataplane_app` usable against the fixture
// schema built by schemaSQL above.
//
// Why a test fixture needs a database ROLE at all: production code in
// internal/repo now runs its queries through orgscope.WithOrgScope, which
// issues `SET LOCAL ROLE dataplane_app` on every scoped transaction. That role
// is created by infra/postgres/migrations/20260809120000_org_rls_isolation.sql,
// which these fixtures never run — they build a slim subset of init.sql
// instead. Without this helper the first repository call to traverse a scoped
// path fails with `role "dataplane_app" does not exist`, and because every test
// here is behind the `integration` build tag a plain `go test ./...` would
// never surface it.
//
// Grants only — no policies. Enabling RLS here would test the migration rather
// than the repository, and schemaSQL is a reduced schema the real policies do
// not match.
//
// Both exception codes are required on the CREATE ROLE. Roles are cluster-wide,
// so parallel packages/binaries sharing one server race here; with the role
// absent, the losing backend raises unique_violation on pg_authid_rolname_index
// before duplicate_object is ever considered.
func grantScopedRuntimeRole(ctx context.Context, t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	if _, err := pool.Exec(ctx, `
		DO $role$
		BEGIN
		    CREATE ROLE dataplane_app NOLOGIN NOSUPERUSER NOBYPASSRLS;
		EXCEPTION WHEN duplicate_object OR unique_violation THEN
		    NULL;
		END
		$role$;

		GRANT USAGE ON SCHEMA public TO dataplane_app;
		GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO dataplane_app;
		GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO dataplane_app;
	`); err != nil {
		t.Fatalf("grant scoped runtime role: %v", err)
	}
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

// TestDocumentDateRoundTripAndPreservedOnUnrelatedUpdate covers P2-3: a
// connector-supplied document_date persists, an omitted one leaves the column
// NULL rather than defaulting to something that would look like a real date,
// and a later re-ingest that doesn't know about this field must not blank out
// a previously-known date -- exactly the failure mode COALESCE in the repo's
// update paths exists to prevent.
func TestDocumentDateRoundTripAndPreservedOnUnrelatedUpdate(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()

	sourceModified := time.Date(2024, 3, 1, 12, 0, 0, 0, time.UTC)
	withDate, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID:        "org-date",
		Source:       "sharepoint",
		Type:         "sharepoint_file",
		Title:        "Q1 Report",
		Content:      "body",
		DocumentDate: &sourceModified,
	})
	if err != nil {
		t.Fatalf("create with document_date: %v", err)
	}
	got, err := r.Get(ctx, "org-date", withDate.Document.DocumentID, "", nil)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if got.DocumentDate == nil || !got.DocumentDate.Equal(sourceModified) {
		t.Fatalf("document_date = %v, want %v", got.DocumentDate, sourceModified)
	}

	withoutDate, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID:   "org-date",
		Source:  "quarry",
		Type:    "web_page",
		Title:   "Undated crawl",
		Content: "body",
	})
	if err != nil {
		t.Fatalf("create without document_date: %v", err)
	}
	gotUndated, err := r.Get(ctx, "org-date", withoutDate.Document.DocumentID, "", nil)
	if err != nil {
		t.Fatalf("get undated: %v", err)
	}
	if gotUndated.DocumentDate != nil {
		t.Fatalf("expected a nil document_date to stay NULL, got %v", gotUndated.DocumentDate)
	}

	// A re-ingest that supplies no document_date (idempotency_key drives the
	// content-refresh path) must not clobber the value set above.
	refreshInput := model.CreateDocumentInput{
		OrgID:          "org-date",
		Source:         "sharepoint",
		Type:           "sharepoint_file",
		Title:          "Q1 Report",
		Content:        "body v2",
		IdempotencyKey: "q1-report",
	}
	first, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID:          "org-date",
		Source:         "sharepoint",
		Type:           "sharepoint_file",
		Title:          "Q1 Report",
		Content:        "body v1",
		IdempotencyKey: "q1-report",
		DocumentDate:   &sourceModified,
	})
	if err != nil {
		t.Fatalf("create idempotent v1: %v", err)
	}
	if first.Document.DocumentDate == nil {
		t.Fatal("v1 lost its document_date immediately after create")
	}
	refreshed, err := r.Create(ctx, refreshInput)
	if err != nil {
		t.Fatalf("create idempotent v2 (refresh): %v", err)
	}
	if !refreshed.Updated {
		t.Fatal("expected the content change to trigger the refresh path")
	}
	if refreshed.Document.DocumentDate == nil || !refreshed.Document.DocumentDate.Equal(sourceModified) {
		t.Fatalf("refresh with no document_date clobbered the stored value: got %v, want %v",
			refreshed.Document.DocumentDate, sourceModified)
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

	// Same key, BYTE-IDENTICAL content — a true no-op replay (e.g. a client
	// retrying after a dropped response). Must return the existing row
	// verbatim: Reused=true, same doc_id, unchanged title.
	replay, err := r.Create(ctx, input)
	if err != nil {
		t.Fatalf("replay create: %v", err)
	}
	if !replay.Reused {
		t.Errorf("byte-identical re-submission should be reused (Reused=%v Updated=%v)", replay.Reused, replay.Updated)
	}
	if replay.Document.DocumentID != first.Document.DocumentID {
		t.Errorf("idempotent replay returned different doc_id")
	}
	if replay.Document.Title != "First" {
		t.Errorf("expected original title on true replay, got %q", replay.Document.Title)
	}

	// Same key, DIFFERENT content — a re-ingest. documents-api's documented
	// contract (see events.DocumentUpdatedEvent and CreateWithOutbox, verified
	// by TestCreateUpdateDeleteLifecycleOutboxIsAtomic) is to refresh the row
	// in place and report Updated=true — never silently discard the new
	// content, and never insert a duplicate row for the same key.
	input.Title = "Second"
	input.Content = "C2"
	second, err := r.Create(ctx, input)
	if err != nil {
		t.Fatalf("second create: %v", err)
	}
	if second.Reused {
		t.Errorf("content-changed re-ingest must not report Reused")
	}
	if !second.Updated {
		t.Errorf("content-changed re-ingest should report Updated")
	}
	if second.Document.DocumentID != first.Document.DocumentID {
		t.Errorf("idempotent create returned different doc_id")
	}
	if second.Document.Title != "Second" {
		t.Errorf("expected refreshed title on content-changed re-ingest, got %q", second.Document.Title)
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

// Classic IDOR probe: an org-B caller who has obtained (guessed, leaked,
// enumerated) org-A's real document_id must not be able to fetch it by
// supplying their own org_id — Get's WHERE clause must reject on org_id
// mismatch, not just on a missing/garbage document_id. This is the read-path
// counterpart to TestSoftDeleteOrgScope (write-path) and TestListOrgScoped
// (collection-path); direct by-ID GET was previously untested.
func TestGetCrossOrgReturnsNotFound(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewDocumentRepo(pool)
	ctx := context.Background()

	doc, err := r.Create(ctx, model.CreateDocumentInput{
		OrgID: "org-A", Source: "s", Type: "t", Title: "A-secret", Content: "confidential", Visibility: "org",
	})
	if err != nil {
		t.Fatalf("create: %v", err)
	}

	// Org B supplies org A's real document_id as their own — must not resolve,
	// not even to prove existence. No viewer/grant filtering is in play here
	// (empty viewerID = legacy org-scoped path), isolating org_id as the only
	// variable under test.
	if _, err := r.Get(ctx, "org-B", doc.Document.DocumentID, "", nil); err == nil {
		t.Fatal("cross-org Get by known document_id succeeded — IDOR leak")
	} else if !errors.Is(err, pgx.ErrNoRows) {
		t.Fatalf("cross-org Get error = %v, want pgx.ErrNoRows", err)
	}

	// Sanity: the same call from the owning org still resolves, so the
	// rejection above is attributable to org_id and not a broken fixture.
	if got, err := r.Get(ctx, "org-A", doc.Document.DocumentID, "", nil); err != nil || got.DocumentID == "" {
		t.Fatalf("same-org Get failed: got=%v err=%v", got, err)
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

func TestSourceObjectLifecycleOutboxIsAtomic(t *testing.T) {
	pool, cleanup := setupPostgres(t)
	defer cleanup()

	r := repo.NewSourceObjectRepo(pool)
	ctx := context.Background()
	input := model.UpsertSourceObjectInput{
		OrgID: "org-source-outbox", Connector: "fixture", Source: "isolated://fixture",
		ExternalID: "source-1", Name: "source-1.txt", ContentHash: "hash-1",
	}
	created, err := r.UpsertWithOutbox(
		ctx,
		input,
		"dataplane.source_objects.changed",
		"user-source",
	)
	if err != nil || !created.Inserted {
		t.Fatalf("source upsert with outbox: result=%+v err=%v", created, err)
	}
	var count int
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM documents_outbox").Scan(&count); err != nil || count != 1 {
		t.Fatalf("source outbox count=%d err=%v", count, err)
	}
	if _, err := r.SoftDeleteWithOutbox(
		ctx,
		model.DeleteSourceObjectInput{OrgID: input.OrgID, SourceObjectID: created.SourceObject.SourceObjectID},
		"dataplane.source_objects.deleted",
		"user-source",
	); err != nil {
		t.Fatalf("source delete with outbox: %v", err)
	}
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM documents_outbox").Scan(&count); err != nil || count != 2 {
		t.Fatalf("source delete outbox count=%d err=%v", count, err)
	}

	if _, err := pool.Exec(ctx, "DROP TABLE documents_outbox"); err != nil {
		t.Fatalf("drop outbox for rollback proof: %v", err)
	}
	input.ExternalID = "source-rollback"
	input.Name = "source-rollback.txt"
	if _, err := r.UpsertWithOutbox(ctx, input, "dataplane.source_objects.changed", "user-source"); err == nil {
		t.Fatal("source-object upsert succeeded without durable outbox")
	}
	if err := pool.QueryRow(ctx, "SELECT COUNT(*) FROM source_objects WHERE external_id='source-rollback'").Scan(&count); err != nil || count != 0 {
		t.Fatalf("source-object commit escaped failed outbox count=%d err=%v", count, err)
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
