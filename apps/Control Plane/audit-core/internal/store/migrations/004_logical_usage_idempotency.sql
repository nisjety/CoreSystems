-- Durable v2 usage may be republished with a new JetStream sequence after a
-- producer crashes between PubAck and its outbox acknowledgement. Preserve the
-- producer's stable identity and make that replay logically idempotent.
-- Historical rows remain nullable; current durable ingress rejects missing IDs.
ALTER TABLE usage_events
  ADD COLUMN IF NOT EXISTS event_id TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS uq_usage_events_source_event
  ON usage_events (source_bus, event_id)
  WHERE source_bus IS NOT NULL AND event_id IS NOT NULL;
