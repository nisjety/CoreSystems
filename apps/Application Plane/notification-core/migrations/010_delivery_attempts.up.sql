-- Durable delivery attempts are control metadata only. Notification payloads
-- remain in notification_requests and are never copied into this table, which
-- keeps ZDR attempt history content-free.
CREATE TABLE IF NOT EXISTS notification_delivery_attempts (
    id                       TEXT PRIMARY KEY,
    notification_id          TEXT NOT NULL REFERENCES notification_requests(id) ON DELETE CASCADE,
    attempt_number           INTEGER NOT NULL CHECK (attempt_number > 0),
    status                   TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'claimed', 'sent_unconfirmed', 'acknowledged', 'failed', 'unknown')),
    worker_id                TEXT NOT NULL DEFAULT '',
    lease_expires_at         TIMESTAMPTZ,
    provider_request_id      TEXT NOT NULL DEFAULT '',
    provider_receipt_digest  TEXT NOT NULL DEFAULT '',
    error_code               TEXT NOT NULL DEFAULT '',
    created_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    submitted_at             TIMESTAMPTZ,
    acknowledged_at          TIMESTAMPTZ,
    UNIQUE (notification_id, attempt_number)
);

CREATE INDEX IF NOT EXISTS notification_delivery_attempts_claim_idx
    ON notification_delivery_attempts (status, lease_expires_at, created_at)
    WHERE status IN ('pending', 'claimed');

CREATE UNIQUE INDEX IF NOT EXISTS notification_delivery_attempts_provider_request_idx
    ON notification_delivery_attempts (provider_request_id)
    WHERE provider_request_id <> '';
