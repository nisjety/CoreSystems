-- Secure-MVP tenant boundary for notification data.
--
-- Existing rows cannot be assigned to a tenant safely from recipient_id or
-- payload alone. They remain quarantined with organization_id = NULL and are
-- never returned by the new org-scoped repositories. The NOT VALID checks are
-- still enforced for every new/updated row while allowing operators to audit
-- and reconcile historical rows from an authoritative Control Plane export.

ALTER TABLE notification_requests
    ADD COLUMN IF NOT EXISTS organization_id TEXT,
    ADD COLUMN IF NOT EXISTS recipient_kind TEXT,
    ADD COLUMN IF NOT EXISTS request_sha256 TEXT;

ALTER TABLE notification_requests
    ADD CONSTRAINT notification_requests_organization_required
        CHECK (organization_id IS NOT NULL AND btrim(organization_id) <> '') NOT VALID,
    ADD CONSTRAINT notification_requests_recipient_kind_user
        CHECK (recipient_kind = 'user') NOT VALID,
    ADD CONSTRAINT notification_requests_request_sha256_valid
        CHECK (request_sha256 ~ '^[0-9a-f]{64}$') NOT VALID;

DROP INDEX IF EXISTS notification_requests_idempotency_key_unique;
CREATE UNIQUE INDEX IF NOT EXISTS notification_requests_org_idempotency_unique
    ON notification_requests (organization_id, idempotency_key)
    WHERE organization_id IS NOT NULL
      AND idempotency_key IS NOT NULL
      AND btrim(idempotency_key) <> '';

CREATE INDEX IF NOT EXISTS notification_requests_org_recipient_created_idx
    ON notification_requests (organization_id, recipient_id, created_at DESC)
    WHERE organization_id IS NOT NULL;

ALTER TABLE notification_feed_items
    ADD COLUMN IF NOT EXISTS organization_id TEXT,
    ADD COLUMN IF NOT EXISTS submitted_at TIMESTAMPTZ;

ALTER TABLE notification_feed_items
    ALTER COLUMN delivery_status SET DEFAULT 'submitted',
    ALTER COLUMN delivered_at DROP NOT NULL;

ALTER TABLE notification_feed_items
    ADD CONSTRAINT notification_feed_items_organization_required
        CHECK (organization_id IS NOT NULL AND btrim(organization_id) <> '') NOT VALID;

DROP INDEX IF EXISTS notification_feed_items_tx_unique;
CREATE UNIQUE INDEX IF NOT EXISTS notification_feed_items_org_tx_unique
    ON notification_feed_items (organization_id, recipient_id, channel, provider_transaction_id)
    WHERE organization_id IS NOT NULL AND provider_transaction_id <> '';

CREATE INDEX IF NOT EXISTS notification_feed_items_org_recipient_delivered_idx
    ON notification_feed_items (organization_id, recipient_id, delivered_at DESC)
    WHERE organization_id IS NOT NULL AND archived = FALSE;

CREATE INDEX IF NOT EXISTS notification_feed_items_org_recipient_submitted_idx
    ON notification_feed_items (organization_id, recipient_id, submitted_at DESC)
    WHERE organization_id IS NOT NULL AND archived = FALSE;

ALTER TABLE notification_preferences
    ADD COLUMN IF NOT EXISTS organization_id TEXT;

ALTER TABLE notification_preferences
    ADD CONSTRAINT notification_preferences_organization_required
        CHECK (organization_id IS NOT NULL AND btrim(organization_id) <> '') NOT VALID;

ALTER TABLE notification_preferences
    DROP CONSTRAINT IF EXISTS notification_preferences_pkey;

CREATE UNIQUE INDEX IF NOT EXISTS notification_preferences_org_user_event_channel_unique
    ON notification_preferences (organization_id, user_id, event_type, channel)
    WHERE organization_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS notification_preferences_org_user_idx
    ON notification_preferences (organization_id, user_id)
    WHERE organization_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS notification_subscriber_memberships (
    organization_id  TEXT        NOT NULL,
    user_id           TEXT        NOT NULL,
    provider_subscriber_id TEXT   NOT NULL,
    role              TEXT        NOT NULL DEFAULT '',
    status            TEXT        NOT NULL,
    authority_revision BIGINT,
    source_event_id   TEXT,
    occurred_at       TIMESTAMPTZ NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (organization_id, user_id),
    CONSTRAINT notification_subscriber_memberships_status_check
        CHECK (status IN ('active', 'removed')),
    CONSTRAINT notification_subscriber_memberships_org_check
        CHECK (btrim(organization_id) <> ''),
    CONSTRAINT notification_subscriber_memberships_user_check
        CHECK (btrim(user_id) <> '')
);

CREATE UNIQUE INDEX IF NOT EXISTS notification_subscriber_memberships_provider_id_unique
    ON notification_subscriber_memberships (provider_subscriber_id);

CREATE INDEX IF NOT EXISTS notification_subscriber_memberships_active_idx
    ON notification_subscriber_memberships (organization_id, user_id)
    WHERE status = 'active';

CREATE OR REPLACE VIEW notification_legacy_unscoped_requests AS
SELECT id, recipient_id, type, status, created_at
FROM notification_requests
WHERE organization_id IS NULL;

CREATE OR REPLACE VIEW notification_legacy_unscoped_feed_items AS
SELECT id, recipient_id, event_type, channel, delivery_status, submitted_at, delivered_at
FROM notification_feed_items
WHERE organization_id IS NULL;

CREATE OR REPLACE VIEW notification_legacy_unscoped_preferences AS
SELECT user_id, event_type, channel, enabled, updated_at
FROM notification_preferences
WHERE organization_id IS NULL;
