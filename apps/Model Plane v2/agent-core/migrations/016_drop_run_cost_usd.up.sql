-- Phase 2: Drop total_cost_usd from agent_runs.
-- Cost tracking is now owned by cost-core-v2 (VELION_COST stream / cost_ledger table).
-- agent-core emits cost events via cost_client.record_cost() instead of storing locally.
DROP INDEX IF EXISTS idx_agent_runs_cost;
ALTER TABLE agent_runs DROP COLUMN IF EXISTS total_cost_usd;
