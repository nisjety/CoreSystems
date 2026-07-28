ALTER TABLE mcp_oauth_tokens
    DROP COLUMN IF EXISTS token_endpoint,
    DROP COLUMN IF EXISTS client_id;
