-- Migration: 007_api_keys.up.sql
-- User-managed API keys for programmatic access

CREATE TABLE IF NOT EXISTS user_api_keys (
    id           TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    user_id      TEXT NOT NULL,
    name         VARCHAR(255) NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    key_hash     VARCHAR(255) NOT NULL,   -- bcrypt hash of the raw key (never stored in plaintext)
    key_prefix   VARCHAR(16) NOT NULL,    -- first chars shown in UI, e.g. "sk_a1b2c3d4"
    scopes       TEXT[] NOT NULL DEFAULT '{}',
    expires_at   TIMESTAMP WITH TIME ZONE,
    revoked_at   TIMESTAMP WITH TIME ZONE,
    last_used_at TIMESTAMP WITH TIME ZONE,
    created_at   TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_api_keys_user_id ON user_api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_user_api_keys_prefix  ON user_api_keys(key_prefix);
