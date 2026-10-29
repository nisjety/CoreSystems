-- Composite-keyed sync checkpoint per (tenant, endpoint, source).
-- The `source` column is what lets ERP REST and DataMart watermarks coexist.

CREATE TABLE IF NOT EXISTS vsv2.sync_checkpoint (
    tenant_id                TEXT NOT NULL,
    endpoint                 TEXT NOT NULL,
    source                   TEXT NOT NULL CHECK (source IN ('erp_rest','datamart_graphql','expense_api')),

    -- ERP REST watermark (lastModifiedDateTime)
    last_modified_watermark  TIMESTAMPTZ,

    -- DataMart watermark + cursor (updatedInDataMartDateTime, lastId)
    last_updated_in_dm       TIMESTAMPTZ,
    last_id                  TEXT,

    last_sync_at             TIMESTAMPTZ,
    records_seen             BIGINT NOT NULL DEFAULT 0,

    created_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (tenant_id, endpoint, source)
);

CREATE INDEX IF NOT EXISTS sync_checkpoint_endpoint_idx
    ON vsv2.sync_checkpoint (endpoint);

CREATE INDEX IF NOT EXISTS sync_checkpoint_tenant_idx
    ON vsv2.sync_checkpoint (tenant_id);
