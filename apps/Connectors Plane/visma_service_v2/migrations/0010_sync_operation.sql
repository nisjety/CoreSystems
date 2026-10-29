CREATE TABLE IF NOT EXISTS vsv2.sync_operation (
    operation_id         UUID PRIMARY KEY,
    kind                 TEXT NOT NULL CHECK (kind IN ('backfill', 'schedule_trigger')),
    tenant_id            TEXT NOT NULL,
    schedule_id          UUID REFERENCES vsv2.sync_schedule(schedule_id),
    endpoints            JSONB NOT NULL,
    date_from            TIMESTAMPTZ,
    date_to              TIMESTAMPTZ,
    status               TEXT NOT NULL CHECK (status IN ('pending','running','succeeded','failed','partial_success')),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at           TIMESTAMPTZ,
    finished_at          TIMESTAMPTZ,
    records_synced       BIGINT NOT NULL DEFAULT 0,
    attempted_endpoints  INTEGER NOT NULL DEFAULT 0,
    succeeded_endpoints  INTEGER NOT NULL DEFAULT 0,
    failed_endpoints     JSONB NOT NULL DEFAULT '[]'::jsonb,
    error                TEXT,
    CONSTRAINT sync_operation_kind_schedule_check CHECK (
        (kind = 'backfill' AND schedule_id IS NULL)
        OR (kind = 'schedule_trigger' AND schedule_id IS NOT NULL)
    ),
    CONSTRAINT sync_operation_endpoints_array CHECK (jsonb_typeof(endpoints) = 'array'),
    CONSTRAINT sync_operation_failed_endpoints_array CHECK (jsonb_typeof(failed_endpoints) = 'array')
);

CREATE INDEX IF NOT EXISTS sync_operation_tenant_created_idx
    ON vsv2.sync_operation (tenant_id, created_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS sync_operation_active_backfill_idx
    ON vsv2.sync_operation (kind)
    WHERE kind = 'backfill' AND status IN ('pending', 'running');

CREATE UNIQUE INDEX IF NOT EXISTS sync_operation_active_schedule_trigger_idx
    ON vsv2.sync_operation (schedule_id)
    WHERE kind = 'schedule_trigger' AND status IN ('pending', 'running');