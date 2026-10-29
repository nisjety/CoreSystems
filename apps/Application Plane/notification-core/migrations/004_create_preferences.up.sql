-- U5-2: per-user notification preferences.
--
-- Each row is a single (user, event_type, channel) opt-in/out toggle.
-- When a user opens /profile/notifications they see one row per known
-- (event_type, channel) combination, defaulting to the org-level channel
-- config (see 005). Writes here also sync to Novu via PATCH /v1/subscribers/
-- {id}/preferences/{workflowId}.
CREATE TABLE IF NOT EXISTS notification_preferences (
    user_id    TEXT        NOT NULL,
    event_type TEXT        NOT NULL,
    channel    TEXT        NOT NULL,
    enabled    BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, event_type, channel)
);

CREATE INDEX IF NOT EXISTS notification_preferences_user_id_idx
    ON notification_preferences (user_id);
