-- Durable delivery state for owner-plane ticket-operation events. Existing
-- conversation events keep their prior semantics; the dispatcher selects only
-- ticket.created rows carrying an operation_id.
ALTER TABLE conversation_events
    ADD COLUMN IF NOT EXISTS delivery_attempts INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS lease_owner TEXT,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN IF NOT EXISTS last_delivery_error TEXT;

CREATE INDEX IF NOT EXISTS conversation_events_ticket_operation_delivery_idx
    ON conversation_events (next_attempt_at ASC, created_at ASC)
    WHERE published_at IS NULL
      AND type = 'ticket.created'
      AND payload ? 'operation_id';
