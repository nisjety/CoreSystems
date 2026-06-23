-- Reverse 002: move the table back to `public` and drop the dedicated schema.
-- Idempotent and safe if the table never moved.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_schema = 'insight_core' AND table_name = 'insight_metric_events'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.tables
        WHERE table_schema = 'public' AND table_name = 'insight_metric_events'
    ) THEN
        ALTER TABLE insight_core.insight_metric_events SET SCHEMA public;
    END IF;
END
$$;

DROP SCHEMA IF EXISTS insight_core CASCADE;
