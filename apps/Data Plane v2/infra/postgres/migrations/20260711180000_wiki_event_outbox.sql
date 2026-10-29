-- Atomic, leased delivery of signed wiki publication intents. The owning wiki
-- page/version transaction inserts this row before commit; workers only mark it
-- delivered after a JetStream PubAck.
CREATE TABLE IF NOT EXISTS wiki_event_outbox (
    outbox_id BIGSERIAL PRIMARY KEY,
    org_id TEXT NOT NULL,
    user_id TEXT,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
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
    CONSTRAINT wiki_event_outbox_identity_check CHECK (
        char_length(btrim(org_id)) > 0
        AND (user_id IS NULL OR char_length(btrim(user_id)) > 0)
        AND event_type = 'dataplane.wiki.version.published'
        AND char_length(idempotency_key) BETWEEN 8 AND 160
        AND idempotency_key = btrim(idempotency_key)
    ),
    CONSTRAINT wiki_event_outbox_payload_check CHECK (
        jsonb_typeof(payload) = 'object'
        AND payload ->> 'org_id' = org_id
        AND payload ->> 'zdr' = 'false'
        AND payload ? 'page_id'
        AND payload ? 'version_id'
        AND payload ? 'content'
        AND (
            (user_id IS NULL AND NOT (payload ? 'user_id'))
            OR payload ->> 'user_id' = user_id
        )
    ),
    CONSTRAINT wiki_event_outbox_status_check
        CHECK (status IN ('pending', 'leased', 'delivered')),
    CONSTRAINT wiki_event_outbox_attempts_check CHECK (attempts >= 0),
    CONSTRAINT wiki_event_outbox_lease_shape_check CHECK (
        (status = 'leased' AND lease_owner IS NOT NULL AND lease_until IS NOT NULL AND delivered_at IS NULL)
        OR (status = 'pending' AND lease_owner IS NULL AND lease_until IS NULL AND delivered_at IS NULL)
        OR (status = 'delivered' AND lease_owner IS NULL AND lease_until IS NULL AND delivered_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS wiki_event_outbox_pending_idx
    ON wiki_event_outbox (available_at, outbox_id)
    WHERE status IN ('pending', 'leased');

CREATE INDEX IF NOT EXISTS wiki_event_outbox_org_idx
    ON wiki_event_outbox (org_id, created_at DESC);

COMMENT ON TABLE wiki_event_outbox IS
    'Transactional signed wiki event intents. Delivery requires JetStream PubAck; leased rows are retryable after expiry.';
