-- A producer can crash after JetStream PubAck but before advancing its outbox.
-- The explicit event identity makes that retry logically idempotent even after
-- the broker's bounded Msg-Id duplicate window expires. source_bus remains part
-- of the key so independently operated plane brokers cannot collide.
ALTER TABLE audit_events
  ADD COLUMN IF NOT EXISTS event_id TEXT;

ALTER TABLE audit_events
  ADD COLUMN IF NOT EXISTS source_subject TEXT;

ALTER TABLE audit_events
  ADD COLUMN IF NOT EXISTS payload_hash BYTEA;

DROP INDEX IF EXISTS uq_audit_events_source_event;

CREATE UNIQUE INDEX IF NOT EXISTS uq_audit_events_source_subject_event
  ON audit_events (source_bus, source_subject, event_id)
  WHERE source_bus IS NOT NULL AND source_subject IS NOT NULL AND event_id IS NOT NULL;
