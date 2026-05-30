-- Phase P: Add USD cost column to agent_runs
ALTER TABLE agent_runs
    ADD COLUMN IF NOT EXISTS total_cost_usd NUMERIC(12, 6) NOT NULL DEFAULT 0;

-- Index to quickly find the most expensive runs per org
CREATE INDEX IF NOT EXISTS idx_agent_runs_cost
    ON agent_runs (org_id, total_cost_usd DESC)
    WHERE total_cost_usd > 0;
