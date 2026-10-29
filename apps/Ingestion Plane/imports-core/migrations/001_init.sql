CREATE TABLE IF NOT EXISTS import_jobs (
    id UUID PRIMARY KEY,
    org_id TEXT NOT NULL,
    user_id TEXT,
    source_type TEXT NOT NULL,
    status TEXT NOT NULL,
    total_items INTEGER NOT NULL DEFAULT 0,
    processed_items INTEGER NOT NULL DEFAULT 0,
    failed_items INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS import_job_items (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES import_jobs(id) ON DELETE CASCADE,
    source_id TEXT,
    source_name TEXT,
    status TEXT NOT NULL,
    error_message TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_import_jobs_org_status ON import_jobs(org_id, status);
CREATE INDEX IF NOT EXISTS idx_import_job_items_job_status ON import_job_items(job_id, status);
