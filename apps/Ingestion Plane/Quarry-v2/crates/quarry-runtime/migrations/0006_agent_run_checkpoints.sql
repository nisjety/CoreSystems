-- Durable browser-agent continuation descriptors.
-- Browser sessions remain process-local; this table records enough verified,
-- tenant-bound state to reacquire a session after restart without pretending
-- that an in-flight action was completed.
CREATE TABLE IF NOT EXISTS quarry_agent_run_checkpoints (
    org_id      TEXT        NOT NULL,
    run_id      TEXT        NOT NULL,
    profile_id  TEXT        NOT NULL,
    step        INT         NOT NULL DEFAULT 0,
    current_url TEXT       NOT NULL DEFAULT '',
    page_hash   TEXT        NOT NULL DEFAULT '',
    state       JSONB       NOT NULL,
    status      TEXT        NOT NULL DEFAULT 'active',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, run_id),
    CHECK (status IN ('active', 'closed'))
);

CREATE INDEX IF NOT EXISTS quarry_agent_run_checkpoints_recent_idx
    ON quarry_agent_run_checkpoints (org_id, updated_at DESC);
