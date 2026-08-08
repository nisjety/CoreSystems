CREATE TABLE IF NOT EXISTS conversation_drafts (
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    body_text TEXT NOT NULL,
    internal BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, conversation_id, user_id)
);

CREATE INDEX IF NOT EXISTS conversation_drafts_org_user_updated_idx
    ON conversation_drafts (org_id, user_id, updated_at DESC);
