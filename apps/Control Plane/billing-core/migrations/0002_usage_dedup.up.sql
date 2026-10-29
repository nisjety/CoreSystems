CREATE TABLE IF NOT EXISTS billing_usage_dedup (
    event_id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    metric TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_billing_usage_dedup_org_metric
    ON billing_usage_dedup (org_id, metric);
