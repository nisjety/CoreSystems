-- Wave-3.2 batch:
--   1. §16.3.7  GIN index on documents.extraction_trace + documents.metadata
--              + retrieval_runs.filters_json + retrieval_runs.mode_mix
--              so admin trace queries become indexed.
--   2. §16.1.3  Add retrieval_runs.zdr_actions_applied JSONB so we record the
--              ZDR actions that were actually executed, not just the requested
--              mode (e.g., classifier kicked in even though zdr_mode=disabled).
--   3. §16.5.3  admin_audit_log table — admin endpoints (orphan cleanup,
--              cohort sweeps, hard delete, reindex) must leave a trail.

-- ── §16.3.7  GIN indexes on existing JSONB columns ──────────────────────────
CREATE INDEX IF NOT EXISTS idx_documents_extraction_trace_gin
    ON documents USING GIN (extraction_trace);

CREATE INDEX IF NOT EXISTS idx_documents_metadata_gin
    ON documents USING GIN (metadata);

CREATE INDEX IF NOT EXISTS idx_retrieval_runs_filters_gin
    ON retrieval_runs USING GIN (filters_json);

CREATE INDEX IF NOT EXISTS idx_retrieval_runs_mode_mix_gin
    ON retrieval_runs USING GIN (mode_mix);

-- ── §16.1.3  zdr_actions_applied column ─────────────────────────────────────
ALTER TABLE retrieval_runs
    ADD COLUMN IF NOT EXISTS zdr_actions_applied JSONB;

COMMENT ON COLUMN retrieval_runs.zdr_actions_applied IS
    'Array of ZDR actions that actually ran on this request '
    '(e.g. ["filter_restricted","strip_pii","ephemeral_trace"]). '
    'Decoupled from zdr_mode which records the requested policy.';

-- ── §16.5.3  admin_audit_log table ──────────────────────────────────────────
CREATE TABLE IF NOT EXISTS admin_audit_log (
    audit_id    BIGSERIAL    PRIMARY KEY,
    org_id      TEXT,
    actor       TEXT         NOT NULL,
    action      TEXT         NOT NULL,
    target_kind TEXT,
    target_id   TEXT,
    request_id  TEXT,
    payload     JSONB,
    outcome     TEXT         NOT NULL DEFAULT 'ok',
    error       TEXT,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_admin_audit_log_time     ON admin_audit_log (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_actor    ON admin_audit_log (actor, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_action   ON admin_audit_log (action, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_org      ON admin_audit_log (org_id, created_at DESC) WHERE org_id IS NOT NULL;

COMMENT ON TABLE admin_audit_log IS
    'Admin-endpoint audit trail: orphan cleanup, sweeps, hard deletes, reindex jobs. '
    'See access_audit_log for user-facing access trail.';
