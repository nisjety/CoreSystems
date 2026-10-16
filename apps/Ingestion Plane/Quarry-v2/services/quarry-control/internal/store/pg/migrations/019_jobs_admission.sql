-- Job admission: an accepted job is leased to the dispatcher for a short time
-- when it is claimed, so a crashed dispatcher's jobs become claimable again and
-- two claims never hand out the same job. The index serves the claim query
-- (status, then per-organization oldest first).
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS claimed_until TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS idx_jobs_status_org_created ON jobs (status, org_id, created_at);
