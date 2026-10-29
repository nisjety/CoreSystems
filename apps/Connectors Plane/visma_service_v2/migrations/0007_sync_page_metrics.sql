-- Durable per-page response metadata for sync performance and rate-limit
-- observability.

CREATE TABLE IF NOT EXISTS vsv2.sync_page_metric (
    metric_id              BIGSERIAL PRIMARY KEY,
    run_id                 UUID NOT NULL REFERENCES vsv2.sync_run(run_id) ON DELETE CASCADE,
    tenant_id              TEXT NOT NULL,
    endpoint               TEXT NOT NULL,
    source                 TEXT NOT NULL CHECK (source IN ('erp_rest','datamart_graphql','expense_api')),
    partition_key          TEXT NOT NULL DEFAULT 'default',
    upstream_path          TEXT NOT NULL,

    page_param             TEXT,
    page_value             BIGINT,
    page_size              BIGINT,

    status                 INTEGER NOT NULL,
    elapsed_ms             BIGINT NOT NULL CHECK (elapsed_ms >= 0),
    records                BIGINT NOT NULL CHECK (records >= 0),
    total_count            BIGINT,
    has_next               BOOLEAN,

    rate_limit_limit       BIGINT,
    rate_limit_remaining   BIGINT,
    rate_limit_reset       BIGINT,
    rate_limit_policy      TEXT,
    retry_after_secs       BIGINT,
    background_mode        TEXT,

    fetched_at             TIMESTAMPTZ NOT NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS sync_page_metric_tenant_endpoint_idx
    ON vsv2.sync_page_metric (tenant_id, endpoint, fetched_at DESC);

CREATE INDEX IF NOT EXISTS sync_page_metric_run_idx
    ON vsv2.sync_page_metric (run_id);

CREATE INDEX IF NOT EXISTS sync_page_metric_rate_remaining_idx
    ON vsv2.sync_page_metric (source, rate_limit_remaining, fetched_at DESC);

CREATE INDEX IF NOT EXISTS sync_page_metric_slow_idx
    ON vsv2.sync_page_metric (source, elapsed_ms DESC, fetched_at DESC);
