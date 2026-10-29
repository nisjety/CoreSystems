-- Preserve enough provenance for marketing_evidence.v1 to be grounded by a
-- model without treating raw provider content or zero-retention material as
-- durable insight data. Existing non-marketing events remain valid: their
-- window and trace fields are nullable/empty.
ALTER TABLE insight_core.insight_metric_events
    ADD COLUMN IF NOT EXISTS time_window_from TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS time_window_to TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS observed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS source_trace_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS retention_class TEXT NOT NULL DEFAULT '';

-- Historical rows were observed when recorded. This avoids a NULL provenance
-- timestamp without asserting a source window or trace that did not exist.
UPDATE insight_core.insight_metric_events
SET observed_at = occurred_at
WHERE observed_at IS NULL;

-- Pre-contract external marketing observations may be useful for historical
-- dashboards, but they lack the mandatory window and trace provenance. Mark
-- them explicitly so the model-only evidence query excludes them.
UPDATE insight_core.insight_metric_events
SET retention_class = 'legacy_unverified'
WHERE connector_type IN (
    'google_analytics_4', 'google_search_console', 'google_ads',
    'meta', 'meta_ads', 'semrush', 'first_party_seo'
)
  AND retention_class = '';

ALTER TABLE insight_core.insight_metric_events
    ALTER COLUMN observed_at SET NOT NULL;
