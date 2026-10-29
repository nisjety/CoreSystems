CREATE TABLE IF NOT EXISTS vsv2.sync_schedule (
    schedule_id UUID PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    name TEXT NOT NULL,
    cron_expression TEXT NOT NULL,
    endpoints JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    disabled_at TIMESTAMPTZ NULL,
    CONSTRAINT sync_schedule_endpoints_is_array CHECK (
        jsonb_typeof(endpoints) = 'array' AND jsonb_array_length(endpoints) > 0
    )
);

CREATE INDEX IF NOT EXISTS idx_sync_schedule_tenant_created
    ON vsv2.sync_schedule (tenant_id, created_at DESC);