-- Email inbound sync cursors (Gmail history.list / Microsoft Graph delta).
-- One row per connection; the cursor is provider-specific:
--   google    → Gmail historyId (users.history.list startHistoryId)
--   microsoft → Graph @odata.deltaLink for /me/mailFolders/inbox/messages/delta
CREATE TABLE IF NOT EXISTS email_sync_state (
    connection_id  TEXT PRIMARY KEY,
    provider_key   TEXT NOT NULL,
    cursor         TEXT NOT NULL DEFAULT '',
    last_synced_at TIMESTAMPTZ,
    last_error     TEXT NOT NULL DEFAULT '',
    failure_count  INTEGER NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_email_sync_state_provider ON email_sync_state (provider_key);
