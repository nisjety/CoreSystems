-- Initialize databases for the Quarry stack
-- Runs automatically when the PostgreSQL container first starts with a fresh volume.
-- The 'quarry' database is pre-created by POSTGRES_DB; temporal DBs are created here.

-- ─────────────────────────────────────────────────────────────────────────────
-- 1. Temporal databases (required by the Temporal server service)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE DATABASE temporal;
CREATE DATABASE temporal_visibility;

-- ─────────────────────────────────────────────────────────────────────────────
-- 2. quarry_jobs schema (required by quarry-api + quarry-worker)
--    The 'quarry' database already exists via POSTGRES_DB env var.
-- ─────────────────────────────────────────────────────────────────────────────
\connect quarry

CREATE TABLE IF NOT EXISTS quarry_jobs (
    id         VARCHAR(64)  PRIMARY KEY,
    status     VARCHAR(32)  NOT NULL DEFAULT 'pending',
    payload    JSONB        NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_quarry_jobs_status
    ON quarry_jobs (status);

CREATE INDEX IF NOT EXISTS idx_quarry_jobs_expires_at
    ON quarry_jobs (expires_at)
    WHERE expires_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_quarry_jobs_created_at
    ON quarry_jobs (created_at DESC);

-- Auto-update updated_at on row change
CREATE OR REPLACE FUNCTION quarry_jobs_set_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

CREATE OR REPLACE TRIGGER trg_quarry_jobs_updated_at
    BEFORE UPDATE ON quarry_jobs
    FOR EACH ROW EXECUTE PROCEDURE quarry_jobs_set_updated_at();

-- ─────────────────────────────────────────────────────────────────────────────
-- 3. job_results schema (Phase 3 pipeline persister — stores enriched results)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS job_results (
    job_id     VARCHAR(64)  PRIMARY KEY,
    result     JSONB        NOT NULL DEFAULT '{}',
    meta       JSONB        NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_job_results_updated_at
    ON job_results (updated_at DESC);

CREATE OR REPLACE FUNCTION job_results_set_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

CREATE OR REPLACE TRIGGER trg_job_results_updated_at
    BEFORE UPDATE ON job_results
    FOR EACH ROW EXECUTE PROCEDURE job_results_set_updated_at();

