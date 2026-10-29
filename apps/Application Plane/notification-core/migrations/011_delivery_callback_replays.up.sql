-- Cross-replica callback replay protection. Nonces are control metadata only
-- and are bounded by expiry; provider payloads are never stored here.
CREATE TABLE IF NOT EXISTS notification_delivery_callback_replays (
    nonce      TEXT PRIMARY KEY,
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS notification_delivery_callback_replays_expiry_idx
    ON notification_delivery_callback_replays (expires_at);
