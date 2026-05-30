-- Ported verbatim from legacy 012_analytics_events.up.sql

CREATE TABLE IF NOT EXISTS analytics_events (
    event_id UUID PRIMARY KEY,
    event_type TEXT NOT NULL,
    org_id TEXT NOT NULL,
    run_id TEXT,
    user_id TEXT,
    agent_id TEXT,
    props JSONB NOT NULL DEFAULT '{}'::jsonb,
    ts TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_analytics_events_org_ts
    ON analytics_events (org_id, ts DESC);
CREATE INDEX IF NOT EXISTS idx_analytics_events_type_ts
    ON analytics_events (event_type, ts DESC);
CREATE INDEX IF NOT EXISTS idx_analytics_events_run
    ON analytics_events (run_id)
    WHERE run_id IS NOT NULL;
