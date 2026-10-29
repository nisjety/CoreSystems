-- A personal, tenant-scoped follow preference. This is intentionally not a
-- notification delivery table: it contains no message/customer content and
-- awaits an Application Plane consumer before notifications are emitted.
CREATE TABLE IF NOT EXISTS conversation_follows (
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, conversation_id, user_id),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS conversation_follows_org_user_idx
    ON conversation_follows (org_id, user_id, created_at DESC);
