-- Add run_id to the jobs table. The orchestrator's jobs dispatcher
-- stamps the Temporal workflow run id here once a job transitions
-- accepted → running, so consumers can poll /v1/runs/{run_id}/events
-- as an alternative to /v1/jobs/{id}/events.
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS run_id TEXT;
CREATE INDEX IF NOT EXISTS idx_jobs_run_id ON jobs(run_id) WHERE run_id IS NOT NULL;
