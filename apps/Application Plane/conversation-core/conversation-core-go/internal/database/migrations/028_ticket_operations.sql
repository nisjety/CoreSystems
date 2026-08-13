-- Owner-plane receipt ledger for the generic `tickets.create` action.
-- This intentionally stores identifiers and a request digest only: ticket
-- content remains in conversation_tickets under its existing retention rules.
CREATE TABLE IF NOT EXISTS conversation_ticket_operations (
    operation_id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    action_id TEXT NOT NULL CHECK (action_id = 'tickets.create'),
    actor_user_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    request_sha256 TEXT NOT NULL CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    ticket_id TEXT NOT NULL,
    audit_event_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('completed', 'unknown')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, idempotency_key)
);

CREATE INDEX IF NOT EXISTS conversation_ticket_operations_ticket_idx
    ON conversation_ticket_operations (org_id, ticket_id, created_at DESC);
