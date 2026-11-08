-- Cycle 23 / cluster #4 part 2 + cluster #14 — schema additions.
--
-- 1. `quarry_sources` — recurring ingestion targets users register.
--    Sourced from /v1/sources POST; refreshed on a schedule.
-- 2. `quarry_benchmarks` — placeholder schema for cycle 28's live
--    benchmark corpus.
-- 3. `quarry_idempotency_keys` — dedup table for D3 / cluster #14.
--    Every POST that mutates control-plane state writes its
--    `(org_id, idempotency_key, route)` tuple here so retries
--    short-circuit instead of double-executing.
--
-- All tables are org-scoped (NOT NULL org_id) and prefixed-indexed
-- for tenant-local query plans.

-- =============================================================================
-- 1. quarry_sources
-- =============================================================================
CREATE TABLE IF NOT EXISTS quarry_sources (
    source_id   TEXT         PRIMARY KEY,   -- ULID, "src_<...>"
    org_id      TEXT         NOT NULL,
    name        TEXT         NOT NULL,
    url         TEXT         NOT NULL,
    kind        TEXT         NOT NULL,       -- "crawl" | "scrape" | "search"
    status      TEXT         NOT NULL DEFAULT 'active',  -- active | paused | deleted
    config      JSONB        NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    deleted_at  TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS quarry_sources_org_idx
    ON quarry_sources (org_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS quarry_sources_name_idx
    ON quarry_sources (org_id, name)
    WHERE deleted_at IS NULL;

-- =============================================================================
-- 2. quarry_benchmarks — placeholder for cycle 28.
-- =============================================================================
CREATE TABLE IF NOT EXISTS quarry_benchmarks (
    benchmark_id   TEXT         PRIMARY KEY,
    org_id         TEXT         NOT NULL,
    name           TEXT         NOT NULL,
    suite          TEXT         NOT NULL,   -- static-html | js-heavy | ...
    baseline       TEXT,                    -- firecrawl-cloud | trafilatura | ...
    status         TEXT         NOT NULL DEFAULT 'pending',
    last_run_at    TIMESTAMPTZ,
    latest_score   DOUBLE PRECISION,
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS quarry_benchmarks_org_idx
    ON quarry_benchmarks (org_id, created_at DESC);

-- =============================================================================
-- 3. quarry_idempotency_keys — D3 / cluster #14.
-- =============================================================================
-- Unique on `(org_id, idempotency_key, route)` so the same key reused
-- on a DIFFERENT route doesn't collide. We hold the request fingerprint
-- (hash of body) so a retry that legitimately re-uses the key on the
-- same route returns the cached response, but a buggy client that
-- re-uses a key with a DIFFERENT body trips a 409.
CREATE TABLE IF NOT EXISTS quarry_idempotency_keys (
    org_id            TEXT         NOT NULL,
    idempotency_key   TEXT         NOT NULL,
    route             TEXT         NOT NULL,
    request_hash      TEXT         NOT NULL,    -- blake3 of canonical body
    response_status   SMALLINT,
    response_body     BYTEA,                    -- cached response (compact)
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    expires_at        TIMESTAMPTZ  NOT NULL,    -- 24h after created_at
    PRIMARY KEY (org_id, idempotency_key, route)
);

-- Cleanup index — a periodic sweeper can DELETE rows where
-- expires_at < NOW() in a single seek.
CREATE INDEX IF NOT EXISTS quarry_idempotency_keys_expires_idx
    ON quarry_idempotency_keys (expires_at)
    WHERE response_status IS NOT NULL;
