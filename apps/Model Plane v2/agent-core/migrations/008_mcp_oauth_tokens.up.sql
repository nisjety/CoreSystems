-- 008_mcp_oauth_tokens.up.sql
-- Persist OAuth access/refresh tokens per MCP server.
-- Tokens are also cached in Redis; this table acts as durable fallback.

CREATE TABLE IF NOT EXISTS mcp_oauth_tokens (
    server_id    TEXT PRIMARY KEY,
    access_token TEXT NOT NULL,
    refresh_token TEXT,
    expires_at   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mcp_oauth_tokens_expires
    ON mcp_oauth_tokens (expires_at);

COMMENT ON TABLE mcp_oauth_tokens IS
    'Durable OAuth 2.0 token store for MCP server connections.';
