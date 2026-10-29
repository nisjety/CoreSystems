-- 0005_endpoint_flags.sql
-- Per-tenant runtime override for endpoint source routing.
-- Resolution priority (see vsv2_common::endpoint::resolve):
--   1. endpoint_flags row (operator override) — DB wins
--   2. registry datamart.tenant_overrides[tenant]
--   3. registry datamart.ready
--   4. registry source (default)

CREATE TABLE IF NOT EXISTS vsv2.endpoint_flags (
    tenant_id  TEXT NOT NULL,
    endpoint   TEXT NOT NULL,
    source     TEXT NOT NULL CHECK (source IN ('erp_rest','datamart_graphql','expense_api')),
    enabled    BOOLEAN NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, endpoint)
);

CREATE INDEX IF NOT EXISTS idx_endpoint_flags_tenant_enabled
    ON vsv2.endpoint_flags(tenant_id, enabled);
