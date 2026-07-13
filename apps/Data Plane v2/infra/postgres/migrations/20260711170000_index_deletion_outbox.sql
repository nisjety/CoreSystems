-- Atomic, retryable index deletion fan-out. The index engine inserts one
-- tenant/document/chunk-scoped intent in the same transaction that deletes the
-- knowledge units, then a leased worker publishes the signed JetStream event.
CREATE TABLE IF NOT EXISTS index_deletion_outbox (
    outbox_id BIGSERIAL PRIMARY KEY,
    org_id TEXT NOT NULL,
    document_id TEXT NOT NULL,
    knowledge_ids JSONB NOT NULL,
    user_id TEXT,
    idempotency_key TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    available_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    lease_owner TEXT,
    lease_until TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT index_deletion_outbox_identity_check CHECK (
        char_length(btrim(org_id)) > 0
        AND char_length(btrim(document_id)) > 0
        AND char_length(idempotency_key) BETWEEN 8 AND 128
        AND idempotency_key = btrim(idempotency_key)
        AND (user_id IS NULL OR char_length(btrim(user_id)) > 0)
    ),
    CONSTRAINT index_deletion_outbox_knowledge_ids_check CHECK (
        jsonb_typeof(knowledge_ids) = 'array'
        AND jsonb_array_length(knowledge_ids) > 0
    ),
    CONSTRAINT index_deletion_outbox_status_check
        CHECK (status IN ('pending', 'leased', 'delivered')),
    CONSTRAINT index_deletion_outbox_attempts_check CHECK (attempts >= 0),
    CONSTRAINT index_deletion_outbox_lease_shape_check CHECK (
        (status = 'leased' AND lease_owner IS NOT NULL AND lease_until IS NOT NULL AND delivered_at IS NULL)
        OR (status = 'pending' AND lease_owner IS NULL AND lease_until IS NULL AND delivered_at IS NULL)
        OR (status = 'delivered' AND lease_owner IS NULL AND lease_until IS NULL AND delivered_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS index_deletion_outbox_pending_idx
    ON index_deletion_outbox (available_at, outbox_id)
    WHERE status IN ('pending', 'leased');

