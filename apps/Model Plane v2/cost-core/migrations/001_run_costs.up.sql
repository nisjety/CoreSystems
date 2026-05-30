CREATE TABLE IF NOT EXISTS run_costs (
    id BIGSERIAL PRIMARY KEY,
    run_id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens BIGINT NOT NULL DEFAULT 0,
    output_tokens BIGINT NOT NULL DEFAULT 0,
    cost_usd NUMERIC(14, 8) NOT NULL DEFAULT 0,
    turn_index INTEGER,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_run_costs_run_id ON run_costs (run_id);
CREATE INDEX IF NOT EXISTS idx_run_costs_org_created
    ON run_costs (org_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_run_costs_org_cost
    ON run_costs (org_id, cost_usd DESC)
    WHERE cost_usd > 0;
