//go:build integration

package handler

// Database fixture for the WS14 handler live tests (room files, shares). It
// needs a throwaway Postgres in TEST_DATABASE_URL and skips without one. The
// schema is the slice of init.sql plus the Space migrations the handlers
// touch; RLS policies are not created (grants only), the same simplification
// the repo package's integration tests make.

import (
	"context"
	"os"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
)

const liveSchemaSQL = `
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
    space_ref    TEXT,
    CONSTRAINT documents_visibility_chk CHECK (visibility IN ('private', 'org', 'shared'))
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_idempotency
    ON documents (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_space_file_sha256
    ON documents (org_id, space_ref, (metadata->>'sha256'))
    WHERE type = 'space_upload' AND deleted_at IS NULL AND space_ref IS NOT NULL;
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
`

// liveDB connects, ensures the schema and returns the repository and the pool.
// The handler tests mint tokens for the fixed testOrg, so rows of that org are
// cleared before and after each test.
func liveDB(t *testing.T) (*repo.DocumentRepo, *pgxpool.Pool) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping documents-api handler live test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	if _, err := pool.Exec(ctx, liveSchemaSQL); err != nil {
		pool.Close()
		t.Fatalf("ensure schema: %v", err)
	}
	clear := func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM documents WHERE org_id = $1`, testOrg)
		_, _ = pool.Exec(context.Background(), `DELETE FROM documents_outbox WHERE org_id = $1`, testOrg)
	}
	clear()
	t.Cleanup(func() {
		clear()
		pool.Close()
	})
	return repo.NewDocumentRepo(pool), pool
}
