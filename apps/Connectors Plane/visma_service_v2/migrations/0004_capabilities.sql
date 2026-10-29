-- Per-tenant per-endpoint capability cache (mirrors v1 Tier-1 capability tracker).
-- Used to avoid retrying endpoints we already know are unsupported for a tenant.

CREATE TABLE IF NOT EXISTS vsv2.endpoint_capability (
    tenant_id      TEXT NOT NULL,
    endpoint       TEXT NOT NULL,
    source         TEXT NOT NULL CHECK (source IN ('erp_rest','datamart_graphql','expense_api')),
    status         TEXT NOT NULL CHECK (status IN ('supported','unsupported','unknown')),
    last_status    INTEGER,
    last_error     TEXT,
    observed_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, endpoint, source)
);
