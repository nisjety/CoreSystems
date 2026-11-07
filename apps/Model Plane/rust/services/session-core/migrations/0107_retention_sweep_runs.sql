-- 0107: retention sweep runs (Wave 3 EP-9 C2; renumbered from 0095 at the W3-19 close so it applies after 0105 in order; 0106 is reserved for EP-12-S14).
--
-- One row per org and sweeper pass (src/retention_sweeper.rs): which policy revision and
-- window applied, how many personal threads were idle past it, how many of those were held,
-- held no content (zero retention) or would be deleted, and the rows the would-delete threads
-- hold per thread-bound table. The owner reads a week of these before the delete mode (C3)
-- is switched on.
--
-- Counts, ids and closed words only: no thread id, title or content is stored, so nothing
-- here needs a retention decision of its own and Zero Data Retention has nothing to refuse.
-- `deleted` stays 0 until C3; `mode` already admits 'enforce' so C3 adds no migration.
-- A pass whose policy was unknown (org-core unreachable, a window out of bounds, a revision
-- that changed mid-pass) writes zero counts and names the reason in `error_code`.
--
-- Additive only: a new, empty table. The reversal is in revert/0107_retention_sweep_runs.down.sql.

CREATE TABLE IF NOT EXISTS retention_sweep_runs (
    id TEXT PRIMARY KEY CHECK (id LIKE 'rsw\_%' AND length(id) <= 64),
    org_id TEXT NOT NULL CHECK (length(org_id) BETWEEN 1 AND 128),
    mode TEXT NOT NULL CHECK (mode IN ('dry_run', 'enforce')),
    policy_revision BIGINT NOT NULL DEFAULT 0 CHECK (policy_revision >= 0),
    -- NULL when the pass failed before a valid window was read.
    window_days INTEGER NULL CHECK (window_days IS NULL OR window_days BETWEEN 30 AND 3650),
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL,
    candidates BIGINT NOT NULL DEFAULT 0 CHECK (candidates >= 0),
    held BIGINT NOT NULL DEFAULT 0 CHECK (held >= 0),
    skipped_zdr BIGINT NOT NULL DEFAULT 0 CHECK (skipped_zdr >= 0),
    would_delete BIGINT NOT NULL DEFAULT 0 CHECK (would_delete >= 0),
    deleted BIGINT NOT NULL DEFAULT 0 CHECK (deleted >= 0),
    -- {"<table>": <rows>} over the would-delete threads; counts only.
    rows_by_table JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(rows_by_table) = 'object'),
    error_code TEXT NULL
        CHECK (error_code IS NULL
               OR error_code IN ('policy_unavailable', 'policy_window_invalid', 'policy_changed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (finished_at >= started_at),
    CHECK (held + skipped_zdr + would_delete = candidates),
    -- A failed pass reports nothing it could not stand behind.
    CHECK (error_code IS NULL OR (candidates = 0 AND deleted = 0))
);

CREATE INDEX IF NOT EXISTS idx_retention_sweep_runs_org
    ON retention_sweep_runs (org_id, started_at DESC);
