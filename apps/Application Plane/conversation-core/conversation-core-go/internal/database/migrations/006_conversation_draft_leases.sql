CREATE TABLE IF NOT EXISTS conversation_draft_leases (
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, conversation_id)
);

CREATE INDEX IF NOT EXISTS conversation_draft_leases_active_idx
    ON conversation_draft_leases (org_id, expires_at);
