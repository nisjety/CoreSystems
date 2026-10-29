-- U5-2 (verevon ui-ux-verevon-gap.md): subscriber identity cache.
--
-- One row per known user. `novu_subscriber_id` is the identifier we pass to
-- Novu when triggering workflows and when listing inbox items. By convention
-- it equals `user_id` from auth-core, but we store it explicitly so we can
-- rotate (e.g. a workspace import that re-keys identities) without breaking
-- the trigger contract.
--
-- The identity columns (email, phone, names, avatar, locale, timezone) are
-- the canonical inputs to Novu subscriber upsert. We sync them to Novu on
-- every NATS `auth.user.*` event so Novu's templates can address users by
-- name / locale without round-tripping to user-core.
CREATE TABLE IF NOT EXISTS notification_subscribers (
    user_id            TEXT        PRIMARY KEY,
    novu_subscriber_id TEXT        NOT NULL,
    email              TEXT        NOT NULL DEFAULT '',
    phone              TEXT        NOT NULL DEFAULT '',
    first_name         TEXT        NOT NULL DEFAULT '',
    last_name          TEXT        NOT NULL DEFAULT '',
    avatar             TEXT        NOT NULL DEFAULT '',
    locale             TEXT        NOT NULL DEFAULT '',
    timezone           TEXT        NOT NULL DEFAULT '',
    -- Optional org membership cache so we can scope by-org queries without
    -- a join through user-core. Best-effort; nullable.
    org_id             TEXT,
    role               TEXT        NOT NULL DEFAULT '',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_synced_at     TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS notification_subscribers_novu_id_unique
    ON notification_subscribers (novu_subscriber_id);

CREATE INDEX IF NOT EXISTS notification_subscribers_org_id_idx
    ON notification_subscribers (org_id)
    WHERE org_id IS NOT NULL;
