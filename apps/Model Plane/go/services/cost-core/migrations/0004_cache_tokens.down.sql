-- Intentionally non-destructive. Cache-token telemetry is retained on
-- rollback; the previous binary safely ignores the additive columns.
SELECT 1;
