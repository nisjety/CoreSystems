-- Reporting resets preserve the accounting ledger and budget history.
CREATE TABLE IF NOT EXISTS usage_reporting_periods (
    id BIGSERIAL PRIMARY KEY,
    org_id TEXT NOT NULL CHECK (org_id <> ''),
    started_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    requested_by TEXT NOT NULL,
    reason TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_reporting_periods_org_start
    ON usage_reporting_periods (org_id, started_at DESC);
