-- Rollback Phase P
DROP INDEX IF EXISTS idx_agent_runs_cost;
ALTER TABLE agent_runs DROP COLUMN IF EXISTS total_cost_usd;
