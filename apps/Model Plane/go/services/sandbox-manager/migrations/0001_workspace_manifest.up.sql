-- S3.3 durable-workspace manifest: the content-addressed file table backing
-- the three-layer workspace model (org read-only / Space / per-run overlay).
-- See apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md §2.
--
-- run_id IS NULL means the Space's merged, durable state; run_id NOT NULL
-- means one run's not-yet-merged overlay (mirrors capability-core's
-- capability_scopes table using revoked_at IS NULL to mean "active" --
-- soft-state via an explicit column, not a second table per state).
CREATE TABLE IF NOT EXISTS workspace_files (
    org_id        TEXT NOT NULL,
    space_id      TEXT NOT NULL,
    run_id        TEXT,
    path          TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    base_hash     TEXT,
    size_bytes    BIGINT NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A PRIMARY KEY constraint cannot contain an expression (COALESCE(run_id,
-- '') below) -- only an index can -- so identity/uniqueness for this table
-- is a UNIQUE INDEX rather than a PRIMARY KEY, the same expression-index
-- shape as capability-core's capability_scopes_active_grant_uq.
CREATE UNIQUE INDEX IF NOT EXISTS workspace_files_identity_uq
    ON workspace_files (org_id, space_id, COALESCE(run_id, ''), path);

CREATE INDEX IF NOT EXISTS workspace_files_space_idx
    ON workspace_files (org_id, space_id) WHERE run_id IS NULL;

CREATE INDEX IF NOT EXISTS workspace_files_run_idx
    ON workspace_files (org_id, space_id, run_id) WHERE run_id IS NOT NULL;
