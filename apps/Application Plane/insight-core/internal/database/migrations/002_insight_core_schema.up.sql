-- insight-core W3 (Phase 4 PR-4): give insight-core its OWN dedicated schema on
-- the shared application-postgres instead of squatting in `public`. This keeps
-- the metric store namespaced away from the other Application-Plane cores that
-- share the database (notification-core, leads-core, ...) so a name collision in
-- `public` can never silently merge tables.
--
-- Idempotent: CREATE SCHEMA IF NOT EXISTS, then relocate the table from `public`
-- only when it is still there (a fresh deploy that ran 001 directly into the new
-- schema would skip the move). The index moves with the table automatically.
CREATE SCHEMA IF NOT EXISTS insight_core;

DO $$
BEGIN
    -- Move the existing public.insight_metric_events into insight_core, but only
    -- if it has not already been relocated (guards re-runs and fresh installs).
    IF EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_schema = 'public' AND table_name = 'insight_metric_events'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_schema = 'insight_core' AND table_name = 'insight_metric_events'
    ) THEN
        ALTER TABLE public.insight_metric_events SET SCHEMA insight_core;
    END IF;
END
$$;

-- Create the table in the dedicated schema if it does not exist yet (covers the
-- fresh-database case where 001 has not produced a public table to relocate).
CREATE TABLE IF NOT EXISTS insight_core.insight_metric_events (
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
    ON insight_core.insight_metric_events (org_id, surface, occurred_at DESC);
