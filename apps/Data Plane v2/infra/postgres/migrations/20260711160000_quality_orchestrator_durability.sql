-- Additive secure-MVP durability for quality evaluations and orchestrator jobs.
-- Both tables are tenant-scoped and use caller-supplied idempotency keys that
-- are unique only within the verified organization boundary.

CREATE TABLE IF NOT EXISTS quality_eval_runs (
    eval_id UUID PRIMARY KEY,
    org_id TEXT NOT NULL,
    strategy TEXT NOT NULL,
    corpus TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    scorecard JSONB,
    error_message TEXT,
    idempotency_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	CONSTRAINT quality_eval_runs_identity_check CHECK (
		char_length(btrim(org_id)) > 0
		AND char_length(idempotency_key) BETWEEN 8 AND 128
		AND idempotency_key = btrim(idempotency_key)
	),
    CONSTRAINT quality_eval_runs_status_check
        CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    CONSTRAINT quality_eval_runs_terminal_shape_check CHECK (
        (status = 'completed' AND scorecard IS NOT NULL AND error_message IS NULL AND finished_at IS NOT NULL)
        OR (status = 'failed' AND error_message IS NOT NULL AND finished_at IS NOT NULL)
        OR (status IN ('pending', 'running') AND finished_at IS NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS quality_eval_runs_org_id_idempotency_key_idx
    ON quality_eval_runs (org_id, idempotency_key);
CREATE INDEX IF NOT EXISTS quality_eval_runs_org_id_created_at_idx
    ON quality_eval_runs (org_id, created_at DESC);

CREATE TABLE IF NOT EXISTS data_orchestrator_jobs (
    job_id UUID PRIMARY KEY,
    org_id TEXT NOT NULL,
    job_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    document_ids JSONB NOT NULL DEFAULT '[]'::JSONB,
    progress INTEGER NOT NULL DEFAULT 0,
    total INTEGER NOT NULL DEFAULT 0,
    result JSONB,
    error_message TEXT,
    idempotency_key TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	CONSTRAINT data_orchestrator_jobs_identity_check CHECK (
		char_length(btrim(org_id)) > 0
		AND char_length(idempotency_key) BETWEEN 8 AND 128
		AND idempotency_key = btrim(idempotency_key)
	),
	CONSTRAINT data_orchestrator_jobs_documents_shape_check
		CHECK (jsonb_typeof(document_ids) = 'array'),
    CONSTRAINT data_orchestrator_jobs_type_check
        CHECK (job_type IN ('reindex', 'graph_build', 'wiki_refresh')),
    CONSTRAINT data_orchestrator_jobs_status_check
        CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    CONSTRAINT data_orchestrator_jobs_progress_check
        CHECK (progress >= 0 AND total >= 0 AND progress <= total),
    CONSTRAINT data_orchestrator_jobs_terminal_shape_check CHECK (
        (status = 'completed' AND error_message IS NULL AND completed_at IS NOT NULL)
        OR (status = 'failed' AND error_message IS NOT NULL AND completed_at IS NOT NULL)
        OR (status IN ('pending', 'running') AND completed_at IS NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS data_orchestrator_jobs_org_id_idempotency_key_idx
    ON data_orchestrator_jobs (org_id, idempotency_key);
CREATE INDEX IF NOT EXISTS data_orchestrator_jobs_org_id_created_at_idx
    ON data_orchestrator_jobs (org_id, created_at DESC);
