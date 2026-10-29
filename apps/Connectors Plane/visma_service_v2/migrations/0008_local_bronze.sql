-- Local bronze sink in Postgres for dry-run validation without BigQuery.
-- Stores latest payload per logical record key and tenant/endpoint/source.

CREATE TABLE IF NOT EXISTS vsv2.local_bronze_record (
    tenant_id        TEXT NOT NULL,
    endpoint         TEXT NOT NULL,
    source           TEXT NOT NULL CHECK (source IN ('erp_rest','datamart_graphql','expense_api')),
    record_key       TEXT NOT NULL,
    record_key_json  JSONB NOT NULL,
    payload          JSONB NOT NULL,
    first_seen_at    TIMESTAMPTZ NOT NULL,
    last_seen_at     TIMESTAMPTZ NOT NULL,
    last_run_id      UUID NOT NULL,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, endpoint, source, record_key)
);

CREATE INDEX IF NOT EXISTS local_bronze_record_endpoint_idx
    ON vsv2.local_bronze_record (tenant_id, endpoint, source);

CREATE INDEX IF NOT EXISTS local_bronze_record_last_seen_idx
    ON vsv2.local_bronze_record (tenant_id, endpoint, last_seen_at DESC);
