-- 004: durable, content-free outbound intent state.
--
-- The unique organization/idempotency key is claimed before any provider call.
-- A process crash or ambiguous provider response therefore becomes `unknown`
-- (or remains `sending`, which callers also treat as unknown) and is never
-- blindly retransmitted. Message persistence, the audit row, AI action outcome,
-- and transition to `submitted` are finalized in one database transaction.

CREATE TABLE IF NOT EXISTS conversation_outbound_intents (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    ai_action_id TEXT NOT NULL DEFAULT '',
    request_fingerprint TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'sending',
    provider TEXT NOT NULL DEFAULT '',
    connection_id TEXT NOT NULL DEFAULT '',
    provider_thread_id TEXT NOT NULL DEFAULT '',
    provider_message_id TEXT NOT NULL DEFAULT '',
    message_id TEXT NOT NULL DEFAULT '',
    error_code TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT conversation_outbound_intents_status_check
        CHECK (status IN ('sending', 'submitted', 'failed', 'unknown')),
    CONSTRAINT conversation_outbound_intents_org_key_unique
        UNIQUE (org_id, idempotency_key)
);

CREATE UNIQUE INDEX IF NOT EXISTS conversation_outbound_intents_org_ai_action_unique
    ON conversation_outbound_intents (org_id, ai_action_id)
    WHERE ai_action_id <> '';

CREATE INDEX IF NOT EXISTS conversation_outbound_intents_reconciliation_idx
    ON conversation_outbound_intents (org_id, status, updated_at);

-- Migration 003's legacy audit table is superseded by the stateful ledger
-- above. Remove its false-success default so any future accidental insert
-- starts conservatively even though current code does not write that table.
ALTER TABLE conversation_ai_action_sends
    ALTER COLUMN status SET DEFAULT 'sending';
