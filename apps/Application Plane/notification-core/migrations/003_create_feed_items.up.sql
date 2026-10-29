-- U5-2: local feed item cache.
--
-- We keep the canonical delivery state on Novu (Novu Inbox API is the source
-- of truth for what was sent and to whom). But verevon's `/notifications`
-- page wants fast list + count queries with our own read/unread/archived
-- semantics, plus the ability to surface notifications that didn't go
-- through a Novu workflow (e.g. internal admin alerts). We mirror the
-- canonical event into this table on dispatch + on inbox webhook callbacks
-- so reads stay snappy.
--
-- Wire contract: matches `Notification` type in
-- verevon/src/lib/notifications/types.ts (so no client changes needed when
-- we extend Novu integration further).
CREATE TABLE IF NOT EXISTS notification_feed_items (
    id                      TEXT        PRIMARY KEY,
    recipient_id            TEXT        NOT NULL,
    -- Workflow / template identifier (Novu workflow_id, e.g.
    -- `control_session.entitlements_changed`).
    event_type              TEXT        NOT NULL,
    -- Channel of THIS row. The same notification can produce multiple
    -- feed items if the workflow targets multiple channels — we record
    -- one row per channel so per-channel read state is meaningful.
    channel                 TEXT        NOT NULL,
    -- Display fields. Rendered from the template at dispatch time;
    -- payload still kept verbatim for the click-through view.
    title                   TEXT        NOT NULL DEFAULT '',
    body                    TEXT        NOT NULL DEFAULT '',
    cta_label               TEXT        NOT NULL DEFAULT '',
    cta_href                TEXT        NOT NULL DEFAULT '',
    payload                 JSONB       NOT NULL DEFAULT '{}'::jsonb,
    actor_id                TEXT        NOT NULL DEFAULT '',
    actor_name              TEXT        NOT NULL DEFAULT '',
    actor_email             TEXT        NOT NULL DEFAULT '',
    actor_avatar            TEXT        NOT NULL DEFAULT '',
    -- User-visible state. `seen` flips when the user opens the dropdown
    -- (clears the badge count); `read` flips when they actually click the
    -- item; `archived` is a soft delete.
    seen                    BOOLEAN     NOT NULL DEFAULT FALSE,
    read                    BOOLEAN     NOT NULL DEFAULT FALSE,
    archived                BOOLEAN     NOT NULL DEFAULT FALSE,
    -- Delivery state from Novu's perspective.
    delivery_status         TEXT        NOT NULL DEFAULT 'delivered',
    -- Timestamps.
    delivered_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    seen_at                 TIMESTAMPTZ,
    read_at                 TIMESTAMPTZ,
    archived_at             TIMESTAMPTZ,
    -- Novu correlation.
    provider                TEXT        NOT NULL DEFAULT 'novu',
    provider_transaction_id TEXT        NOT NULL DEFAULT '',
    source                  TEXT        NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS notification_feed_items_recipient_delivered_idx
    ON notification_feed_items (recipient_id, delivered_at DESC)
    WHERE archived = FALSE;

-- Partial index for fast unseen/unread badge counts.
CREATE INDEX IF NOT EXISTS notification_feed_items_recipient_unseen_idx
    ON notification_feed_items (recipient_id)
    WHERE archived = FALSE AND seen = FALSE;

CREATE INDEX IF NOT EXISTS notification_feed_items_recipient_unread_idx
    ON notification_feed_items (recipient_id)
    WHERE archived = FALSE AND read = FALSE;

CREATE INDEX IF NOT EXISTS notification_feed_items_event_type_idx
    ON notification_feed_items (event_type);

-- Deduplicate via Novu's transaction id when present. The partial index
-- avoids enforcing uniqueness on rows that have no upstream transaction
-- (e.g. locally-published in-app messages).
CREATE UNIQUE INDEX IF NOT EXISTS notification_feed_items_tx_unique
    ON notification_feed_items (recipient_id, channel, provider_transaction_id)
    WHERE provider_transaction_id <> '';
