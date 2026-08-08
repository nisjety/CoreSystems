-- Insight Core: add an optional verified actor projection inside the already
-- mandatory organization tenant boundary. Empty actor_user_id represents
-- organization-level events (for example, inbound customer messages), while a
-- populated value enables a signed-in user to request only their own activity.
ALTER TABLE insight_core.insight_metric_events
    ADD COLUMN IF NOT EXISTS actor_user_id TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_insight_metric_events_org_actor_surface_time
    ON insight_core.insight_metric_events (org_id, actor_user_id, surface, occurred_at DESC)
    WHERE actor_user_id <> '';
