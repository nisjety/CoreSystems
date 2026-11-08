-- Job record v2: who started a job (origin), what state the caller wants it in
-- (desired state and its sequence, for the control loop of WS15.L2), the
-- Temporal workflow id, the deadline and the terminal reason and time.
-- Additive and defaulted: rows written before this migration read as origin
-- 'unknown' and no desired state, and the pre-existing write paths keep working.
-- Every statement is IF NOT EXISTS, so the migration can run twice.
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS origin_type TEXT NOT NULL DEFAULT 'unknown';
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS origin_actor TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS origin_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS created_by TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS desired_state TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS desired_seq BIGINT NOT NULL DEFAULT 0;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS applied_seq BIGINT NOT NULL DEFAULT 0;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS workflow_id TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS deadline_at BIGINT;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS terminal_reason TEXT NOT NULL DEFAULT '';
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS finished_at BIGINT;

-- Serves the control loop: jobs whose desired state is ahead of what was applied.
CREATE INDEX IF NOT EXISTS idx_jobs_pending_control ON jobs (id) WHERE desired_seq > applied_seq;
-- Serves the ongoing list: the non-terminal jobs, oldest first.
CREATE INDEX IF NOT EXISTS idx_jobs_active ON jobs (created_at) WHERE status IN ('accepted', 'running', 'paused');
-- Serves GET /v1/{kind}/jobs?status=a,b per organization.
CREATE INDEX IF NOT EXISTS idx_jobs_org_kind_status ON jobs (org_id, kind, status, created_at DESC);
