ALTER TABLE audit_events
  ADD COLUMN IF NOT EXISTS source_bus TEXT,
  ADD COLUMN IF NOT EXISTS source_stream_sequence BIGINT;

ALTER TABLE audit_events
  DROP CONSTRAINT IF EXISTS audit_events_source_stream_sequence_positive;
ALTER TABLE audit_events
  ADD CONSTRAINT audit_events_source_stream_sequence_positive
  CHECK (source_stream_sequence IS NULL OR source_stream_sequence > 0);

CREATE UNIQUE INDEX IF NOT EXISTS uq_audit_events_jetstream_inbox
  ON audit_events (source_bus, source_stream_sequence)
  WHERE source_bus IS NOT NULL AND source_stream_sequence IS NOT NULL;

ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS source_bus TEXT,
  ADD COLUMN IF NOT EXISTS source_stream_sequence BIGINT;

ALTER TABLE usage_events
  DROP CONSTRAINT IF EXISTS usage_events_source_stream_sequence_positive;
ALTER TABLE usage_events
  ADD CONSTRAINT usage_events_source_stream_sequence_positive
  CHECK (source_stream_sequence IS NULL OR source_stream_sequence > 0);

CREATE UNIQUE INDEX IF NOT EXISTS uq_usage_events_jetstream_inbox
  ON usage_events (source_bus, source_stream_sequence)
  WHERE source_bus IS NOT NULL AND source_stream_sequence IS NOT NULL;
