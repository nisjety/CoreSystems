CREATE TABLE IF NOT EXISTS notification_requests (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT,
    recipient_id TEXT NOT NULL,
    type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    source TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'novu',
    provider_request_id TEXT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    submitted_at TIMESTAMPTZ,
    failed_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS notification_requests_idempotency_key_unique
    ON notification_requests (idempotency_key)
    WHERE idempotency_key IS NOT NULL AND btrim(idempotency_key) <> '';

CREATE INDEX IF NOT EXISTS notification_requests_status_created_at_idx
    ON notification_requests (status, created_at DESC);