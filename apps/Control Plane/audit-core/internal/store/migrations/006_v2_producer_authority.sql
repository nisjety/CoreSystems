-- V2 subjects bind every event to an ACL-scoped producer:
-- verevon.<kind>.v2.<plane>.<producer>.<event-or-op>. Logical retries must
-- remain idempotent even if the event suffix or JetStream sequence changes.
-- Historical v1 rows remain nullable because their subject did not identify a
-- producer and inventing one would create false authority.
ALTER TABLE audit_events
  ADD COLUMN IF NOT EXISTS source_producer TEXT;

ALTER TABLE audit_events
  DROP CONSTRAINT IF EXISTS audit_events_source_producer_format;
ALTER TABLE audit_events
  ADD CONSTRAINT audit_events_source_producer_format
  CHECK (
    source_producer IS NULL OR
    source_producer ~ '^[a-z0-9][a-z0-9_-]{0,63}$'
  );

CREATE UNIQUE INDEX IF NOT EXISTS uq_audit_events_source_producer_event
  ON audit_events (source_bus, source_producer, event_id)
  WHERE source_bus IS NOT NULL AND source_producer IS NOT NULL AND event_id IS NOT NULL;

ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS source_producer TEXT;

ALTER TABLE usage_events
  DROP CONSTRAINT IF EXISTS usage_events_source_producer_format;
ALTER TABLE usage_events
  ADD CONSTRAINT usage_events_source_producer_format
  CHECK (
    source_producer IS NULL OR
    source_producer ~ '^[a-z0-9][a-z0-9_-]{0,63}$'
  );

CREATE UNIQUE INDEX IF NOT EXISTS uq_usage_events_source_producer_event
  ON usage_events (source_bus, source_producer, event_id)
  WHERE source_bus IS NOT NULL AND source_producer IS NOT NULL AND event_id IS NOT NULL;
