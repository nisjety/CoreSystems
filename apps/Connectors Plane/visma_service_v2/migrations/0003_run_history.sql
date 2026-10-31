CREATE TABLE IF NOT EXISTS vsv2.sync_run (
    run_id           UUID PRIMARY KEY,
    tenant_id        TEXT NOT NULL,
    endpoint         TEXT NOT NULL,
    source           TEXT NOT NULL CHECK (source IN ('erp_rest','datamart_graphql','expense_api')),
    status           TEXT NOT NULL CHECK (status IN ('pending','running','succeeded','failed','partial_success')),
    started_at       TIMESTAMPTZ NOT NULL,
    finished_at      TIMESTAMPTZ,
    records_in       BIGINT NOT NULL DEFAULT 0,
    records_written  BIGINT NOT NULL DEFAULT 0,
    error            TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS sync_run_tenant_idx ON vsv2.sync_run (tenant_id);
CREATE INDEX IF NOT EXISTS sync_run_started_idx ON vsv2.sync_run (started_at DESC);

CREATE TABLE IF NOT EXISTS vsv2.sync_table_result (
    run_id           UUID NOT NULL REFERENCES vsv2.sync_run(run_id) ON DELETE CASCADE,
    table_name       TEXT NOT NULL,
    rows_merged      BIGINT NOT NULL DEFAULT 0,
    bytes_written    BIGINT NOT NULL DEFAULT 0,
    bq_job_id        TEXT,
    error            TEXT,
    PRIMARY KEY (run_id, table_name)
);
