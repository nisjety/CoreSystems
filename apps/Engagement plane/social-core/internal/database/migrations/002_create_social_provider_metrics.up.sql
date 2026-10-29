CREATE TABLE IF NOT EXISTS social_provider_metrics (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    connection_id TEXT NOT NULL DEFAULT '',
    provider_key TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    metric_value NUMERIC NOT NULL DEFAULT 0,
    dimensions JSONB NOT NULL DEFAULT '{}'::jsonb,
    snapshot_date DATE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (jsonb_typeof(dimensions) = 'object')
);

-- Dedup key: re-running a snapshot for the same day upserts in place instead
-- of accumulating duplicate rows (jsonb equality is key-order independent).
CREATE UNIQUE INDEX IF NOT EXISTS social_provider_metrics_dedup_unique
    ON social_provider_metrics (org_id, account_id, metric_name, dimensions, snapshot_date);
CREATE INDEX IF NOT EXISTS social_provider_metrics_org_account_date_idx
    ON social_provider_metrics (org_id, account_id, snapshot_date DESC);
