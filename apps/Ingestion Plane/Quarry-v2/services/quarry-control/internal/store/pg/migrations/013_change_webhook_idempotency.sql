-- Change-webhook idempotency (docs/CHANGE_TRACKING.md §"Webhook emission").
--
-- The edge producer signs POST /v1/webhooks/change with an Idempotency-Key
-- header so a retry after an ambiguous failure (connection dropped after the
-- server persisted but before the client saw the 2xx) never double-records
-- a change event. The events table's existing idempotency_key column is only
-- covered by a NON-unique index (001_init.sql), because run-event publishers
-- legitimately reuse keys across runs. Change-webhook deliveries are the one
-- producer whose key must be globally unique, so constrain exactly those rows
-- with a partial unique index instead of forcing uniqueness on everyone.
--
-- The receiver's lookup (store.EventLog.FindByIdempotencyKey) filters on the
-- same type='change_detected' predicate — keep them in lockstep.

CREATE UNIQUE INDEX IF NOT EXISTS events_change_idem_uidx
    ON events (idempotency_key)
    WHERE type = 'change_detected' AND idempotency_key IS NOT NULL AND idempotency_key <> '';
