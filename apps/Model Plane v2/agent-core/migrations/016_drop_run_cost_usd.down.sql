-- Rollback: re-add total_cost_usd column + index (inverse of 009_run_cost_usd.up.sql).
ALTER TABLE agent_runs ADD COLUMN IF NOT EXISTS total_cost_usd NUMERIC(12, 6) NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_agent_runs_cost
    ON agent_runs (org_id, total_cost_usd DESC)
    WHERE total_cost_usd > 0;
