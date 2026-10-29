-- Activity log table for user action tracking
CREATE TABLE IF NOT EXISTS user_activity_log (
    id          TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    user_id     TEXT NOT NULL,
    action      TEXT NOT NULL,
    resource    TEXT NOT NULL DEFAULT '',
    details     JSONB,
    ip_address  TEXT NOT NULL DEFAULT '',
    user_agent  TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_activity_log_user_id_created
    ON user_activity_log (user_id, created_at DESC);
