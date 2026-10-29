-- Cross-tenant job isolation: every job now carries the org that created
-- it, mirroring schedules' org_id (008_schedules_org_id.sql) and sources'
-- org_id (005_cycle23.sql). org_id is stamped server-side from the edge-
-- verified `?org_id` query param — createJob rejects an empty value — so
-- GET /v1/jobs and GET /v1/{kind}/jobs can filter to the caller's own jobs
-- instead of listing every tenant's jobs together. Backfill is '' for any
-- pre-existing rows.
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS org_id TEXT NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS idx_jobs_org_id ON jobs(org_id);
-- Serves both GET /v1/jobs (org_id, created_at) and GET /v1/{kind}/jobs
-- (org_id, kind, created_at) — the leading org_id column covers both.
CREATE INDEX IF NOT EXISTS idx_jobs_org_id_kind_created
    ON jobs (org_id, kind, created_at DESC);
