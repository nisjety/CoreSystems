-- insight-core W3 (PR-3): durable per-org metric events.
-- Phase-1 A was registry-only with an in-memory repo; this table makes recorded
-- metric events survive restarts so briefs can be assembled from real history.
-- The connector registry stays static/in-code (org-independent) and is NOT stored here.
CREATE TABLE IF NOT EXISTS insight_metric_events (
    id             TEXT PRIMARY KEY,
    org_id         TEXT NOT NULL,
    surface        TEXT NOT NULL,
    metric         TEXT NOT NULL,
    value          DOUBLE PRECISION NOT NULL DEFAULT 0,
    unit           TEXT NOT NULL DEFAULT '',
    source         TEXT NOT NULL DEFAULT '',
    connector_type TEXT NOT NULL DEFAULT '',
    dimensions     JSONB NOT NULL DEFAULT '{}'::jsonb,
    occurred_at    TIMESTAMPTZ NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Overview/brief queries filter by org + surface within a time window, newest first.
CREATE INDEX IF NOT EXISTS idx_insight_metric_events_org_surface_time
    ON insight_metric_events (org_id, surface, occurred_at DESC);
