-- OAuth 2.1 + Dynamic Client Registration (RFC 7591) support for
-- mcp_oauth_tokens: refreshing an access token needs the token_endpoint and
-- the client_id DCR minted for this server, neither of which the original
-- schema carried. access_token/refresh_token are encrypted ciphertext
-- (AES-256-GCM, "v1:" prefix, mirroring integration-corev2's vault) from this
-- point on, written only by capability-core's mcpcrypto package — never
-- plaintext at rest.

ALTER TABLE mcp_oauth_tokens
    ADD COLUMN IF NOT EXISTS token_endpoint TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS client_id TEXT NOT NULL DEFAULT '';
