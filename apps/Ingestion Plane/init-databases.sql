-- =============================================================================
-- Ingestion Plane – PostgreSQL initialisation script
-- Runs once when the container starts with a fresh data volume.
-- The container superuser is ingestion_user (POSTGRES_USER).
-- =============================================================================

-- ─────────────────────────────────────────────────────────────────────────────
-- 1. quarry database  (used by Quarry extraction API + worker)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE DATABASE quarry
    WITH OWNER = ingestion_user
         ENCODING = 'UTF8'
         LC_COLLATE = 'en_US.utf8'
         LC_CTYPE   = 'en_US.utf8'
         TEMPLATE   = template0;

\connect quarry

-- Job store for async extraction work
CREATE TABLE IF NOT EXISTS quarry_jobs (
    id         VARCHAR(64)  PRIMARY KEY,
    status     VARCHAR(32)  NOT NULL DEFAULT 'pending',
    payload    JSONB        NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_quarry_jobs_status     ON quarry_jobs (status);
CREATE INDEX IF NOT EXISTS idx_quarry_jobs_expires_at ON quarry_jobs (expires_at)
    WHERE expires_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_quarry_jobs_created_at ON quarry_jobs (created_at DESC);

-- Auto-update updated_at on row change
CREATE OR REPLACE FUNCTION quarry_jobs_set_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_quarry_jobs_updated_at
    BEFORE UPDATE ON quarry_jobs
    FOR EACH ROW EXECUTE PROCEDURE quarry_jobs_set_updated_at();

-- ─────────────────────────────────────────────────────────────────────────────
-- 2. quarry_v2 database  (used by Quarry v2 edge/control/orchestrator)
-- ─────────────────────────────────────────────────────────────────────────────
\connect ingestion_plane_db

SELECT 'CREATE DATABASE quarry_v2 OWNER ingestion_user'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'quarry_v2')\gexec

-- ─────────────────────────────────────────────────────────────────────────────
-- 3. imports database  (used by imports-core / document import pipeline)
-- ─────────────────────────────────────────────────────────────────────────────
\connect ingestion_plane_db

CREATE DATABASE imports
    WITH OWNER = ingestion_user
         ENCODING = 'UTF8'
         LC_COLLATE = 'en_US.utf8'
         LC_CTYPE   = 'en_US.utf8'
         TEMPLATE   = template0;

\connect imports

-- imports-core owns its schema through startup SQL migrations.
-- Keep the database empty here so fresh volumes do not bootstrap an outdated
-- import_jobs table shape that conflicts with the service's UUID-based schema.
-- ─────────────────────────────────────────────────────────────────────────────
-- 4. integration database  (used by integration-core / M365 OAuth + discovery)
-- ─────────────────────────────────────────────────────────────────────────────
\connect ingestion_plane_db

CREATE DATABASE integration
    WITH OWNER = ingestion_user
         ENCODING = 'UTF8'
         LC_COLLATE = 'en_US.utf8'
         LC_CTYPE   = 'en_US.utf8'
         TEMPLATE   = template0;

\connect integration

-- OAuth tokens table (SQLAlchemy will create this, but ensure encoding)
-- Reserved for oauth_tokens, integration_connections, sync_jobs tables
-- which will be auto-created by SQLAlchemy on service startup

-- ─────────────────────────────────────────────────────────────────────────────
-- 5. Temporal databases — MOVED to a dedicated Postgres instance
--    (see velion-gap.md §8.33 G50)
--
--    Previously the `temporal` + `temporal_visibility` databases lived on
--    this shared `ingestion-postgres` instance alongside quarry / quarry_v2
--    / ingestion_plane_db / imports / integration. With only 64MB
--    shared_buffers, Temporal's history-shard scanner consistently blew
--    the cache and produced `GetTransferTasks ... context deadline
--    exceeded` errors. The split moved Temporal to its own
--    `ingestion-temporal-postgres` service (1GB RAM, 256MB shared_buffers,
--    768MB effective_cache_size) — that postgres self-initializes via
--    POSTGRES_DB=temporal, and the temporal auto-setup container creates
--    `temporal_visibility` on first boot. No init script needed there.
--
--    DO NOT re-add `temporal` / `temporal_visibility` to this file unless
--    you also revert the compose split — keeping the two configs in sync
--    matters more than re-creating idempotent DBs that will never be used.
-- ─────────────────────────────────────────────────────────────────────────────
\connect ingestion_plane_db

-- ─────────────────────────────────────────────────────────────────────────────
-- 6. finspo database  (used by finspo-core / SharePoint Graph delta ingest)
--    The service owns its schema through embedded migrations applied at
--    startup (internal/db.ApplyMigrations) — keep the database empty here.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE DATABASE finspo
    WITH OWNER = ingestion_user
         ENCODING = 'UTF8'
         LC_COLLATE = 'en_US.utf8'
         LC_CTYPE   = 'en_US.utf8'
         TEMPLATE   = template0;
