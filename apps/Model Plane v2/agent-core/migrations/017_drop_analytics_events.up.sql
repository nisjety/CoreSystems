-- Phase 2: Drop analytics_events table.
-- Analytics ownership migrated to cost-core-v2. agent-core publishes events via
-- cost_client.emit_event() → POST /v1/analytics/events on cost-core-v2.
DROP TABLE IF EXISTS analytics_events CASCADE;
