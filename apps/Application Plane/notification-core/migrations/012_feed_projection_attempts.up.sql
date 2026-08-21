-- Durable, content-free local Activity/Inbox projection queue. The
-- notification payload remains in notification_requests; this table only
-- carries provider correlation, leases, and receipt state.
CREATE TABLE IF NOT EXISTS notification_feed_projection_attempts (
    id                       TEXT PRIMARY KEY,
    attempt_id               TEXT NOT NULL UNIQUE REFERENCES notification_delivery_attempts(id) ON DELETE CASCADE,
    notification_id          TEXT NOT NULL REFERENCES notification_requests(id) ON DELETE CASCADE,
    provider_request_id      TEXT NOT NULL DEFAULT '',
    provider_receipt_digest  TEXT NOT NULL DEFAULT '',
    status                   TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'claimed', 'projected', 'unknown')),
    delivery_status          TEXT NOT NULL DEFAULT 'submitted'
        CHECK (delivery_status IN ('submitted', 'delivered')),
    worker_id                TEXT NOT NULL DEFAULT '',
    lease_expires_at         TIMESTAMPTZ,
    error_code               TEXT NOT NULL DEFAULT '',
    created_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    submitted_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at             TIMESTAMPTZ,
    next_attempt_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS notification_feed_projection_claim_idx
    ON notification_feed_projection_attempts (status, next_attempt_at, lease_expires_at, created_at)
    WHERE status IN ('pending', 'claimed', 'unknown');
