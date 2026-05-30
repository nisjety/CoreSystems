-- 0005_finetune_jobs.sql
--
-- Wave 7 v1 — fine-tuning job state machine.
--
-- Layout choices:
--   * Lives in session-core's Postgres because session-core already owns
--     long-running run/checkpoint state. Will move to `train-core` when the
--     extraction criteria in gap-model.md §14.9 PAR-30 are hit.
--   * `azure_*` fields are nullable-by-default ('' instead of NULL) so the
--     row can be persisted *before* the provider call returns — protects
--     against orphan Azure jobs on session-core restart.
--   * State machine column is text, not enum: cheap to add new states; the
--     gateway enforces the valid set (queued|running|succeeded|failed|cancelled).
--   * Idempotent CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS.

CREATE TABLE IF NOT EXISTS finetune_jobs (
    job_id                  TEXT PRIMARY KEY,
    org_id                  TEXT NOT NULL,
    agent_id                TEXT NOT NULL,
    base_model              TEXT NOT NULL,

    -- Provider artefacts (populated by the gateway as Azure calls complete).
    azure_file_id           TEXT NOT NULL DEFAULT '',
    azure_job_id            TEXT NOT NULL DEFAULT '',
    fine_tuned_model        TEXT NOT NULL DEFAULT '',
    deployment_name         TEXT NOT NULL DEFAULT '',

    -- Lifecycle.
    status                  TEXT NOT NULL DEFAULT 'queued',
    error_message           TEXT NOT NULL DEFAULT '',

    -- Inputs / accounting.
    hyperparameters_json    JSONB NOT NULL DEFAULT '{}',
    training_example_count  INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd      NUMERIC(10,4) NOT NULL DEFAULT 0,
    actual_cost_usd         NUMERIC(10,4) NOT NULL DEFAULT 0,

    -- Audit.
    created_by              TEXT NOT NULL DEFAULT '',
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at            TIMESTAMPTZ
);

-- Per-org listing in reverse-chronological order.
CREATE INDEX IF NOT EXISTS finetune_jobs_org_idx
    ON finetune_jobs (org_id, created_at DESC);

-- Per-agent listing scoped by org (the UI's most common query).
CREATE INDEX IF NOT EXISTS finetune_jobs_agent_idx
    ON finetune_jobs (org_id, agent_id, created_at DESC);

-- Polling worker scans rows still in flight; partial index keeps it small.
CREATE INDEX IF NOT EXISTS finetune_jobs_active_idx
    ON finetune_jobs (status, updated_at)
    WHERE status IN ('queued', 'running');

-- Azure lookup for the polling worker — sparse on `''`.
CREATE INDEX IF NOT EXISTS finetune_jobs_azure_idx
    ON finetune_jobs (azure_job_id)
    WHERE azure_job_id <> '';
