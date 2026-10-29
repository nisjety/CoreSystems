-- Bind logical usage identity to the producer boundary. A bus-wide event ID is
-- insufficient because multiple scoped publishers can legitimately reuse an
-- identifier. Exact retries retain a payload hash so conflicting reuse fails
-- visibly instead of silently dropping billable usage.
ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS source_subject TEXT;

ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS payload_hash BYTEA;

DROP INDEX IF EXISTS uq_usage_events_source_event;

CREATE UNIQUE INDEX IF NOT EXISTS uq_usage_events_source_subject_event
  ON usage_events (source_bus, source_subject, event_id)
  WHERE source_bus IS NOT NULL AND source_subject IS NOT NULL AND event_id IS NOT NULL;
