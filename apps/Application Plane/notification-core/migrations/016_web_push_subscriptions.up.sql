-- WS16-S6: encrypted Web Push subscriptions. One row is one browser endpoint of one user in one
-- organization. The endpoint and the two client keys are sealed with NOTIFICATION_PUSH_SUBSCRIPTION_KEY
-- before they are written, so a database read yields no usable push capability. A subscription holds no
-- conversation data: every push it receives is the same constant wake-up.
CREATE TABLE IF NOT EXISTS notification_web_push_subscriptions (
    id              TEXT        PRIMARY KEY,
    organization_id TEXT        NOT NULL,
    user_id         TEXT        NOT NULL,
    endpoint_hash   TEXT        NOT NULL CHECK (endpoint_hash ~ '^[0-9a-f]{64}$'),
    endpoint_ct     BYTEA       NOT NULL,
    p256dh_ct       BYTEA       NOT NULL,
    auth_ct         BYTEA       NOT NULL,
    ua_class        TEXT        NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_ok_at      TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ,
    UNIQUE (organization_id, user_id, endpoint_hash)
);

CREATE INDEX IF NOT EXISTS notification_web_push_subscriptions_recipient_idx
    ON notification_web_push_subscriptions (organization_id, user_id, created_at DESC)
    WHERE revoked_at IS NULL;
