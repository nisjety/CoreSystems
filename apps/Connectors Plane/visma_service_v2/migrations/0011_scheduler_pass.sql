-- One row per pass of the built-in scheduler over a tenant's endpoints of one tier.
--
-- The scheduler decides what is due from this table, not from timers in memory, so a restart, a
-- machine that slept, or a second instance does not shift or repeat the schedule. A pass that
-- ended with failures is retried for just the failed endpoints (`retry_of` points at the pass it
-- repeats, `failed_names` lists what is still failing).
--
-- A running pass writes `heartbeat_at` every minute. A pass whose heartbeat has stopped belongs to a
-- process that died (container restart, crash); it is marked failed and `abandoned`, and the next
-- poll runs it again instead of waiting for its interval.
CREATE TABLE IF NOT EXISTS vsv2.scheduler_pass (
    pass_id             UUID PRIMARY KEY,
    tier                TEXT NOT NULL CHECK (tier IN ('hourly', 'daily', 'full')),
    tenant_id           TEXT NOT NULL,
    status              TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    retry_of            UUID REFERENCES vsv2.scheduler_pass(pass_id),
    started_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at         TIMESTAMPTZ,
    attempted_endpoints INTEGER NOT NULL DEFAULT 0,
    failed_names        JSONB NOT NULL DEFAULT '[]'::jsonb,
    records_synced      BIGINT NOT NULL DEFAULT 0,
    error               TEXT,
    heartbeat_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    abandoned           BOOLEAN NOT NULL DEFAULT FALSE
);

-- For a database that created the table before these two columns existed.
ALTER TABLE vsv2.scheduler_pass ADD COLUMN IF NOT EXISTS heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE vsv2.scheduler_pass ADD COLUMN IF NOT EXISTS abandoned BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_scheduler_pass_lookup
    ON vsv2.scheduler_pass (tier, tenant_id, started_at DESC);
