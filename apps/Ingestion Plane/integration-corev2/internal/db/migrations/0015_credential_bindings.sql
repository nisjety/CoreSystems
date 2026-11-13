-- W4-CC-3: credential bindings (sandbox credential injection, custody half).
-- ADR-NEXT-sandbox-credential-injection sections 1 and 4.
--
-- Forward-only (there are no down migrations in this service). A credential binding names ONE
-- host an organisation admin has bound a secret to, the single header the secret may be placed
-- in, and the methods and path prefixes that may be forwarded with it. The secret is sealed with
-- the credential vault (AES-GCM, additional data = the binding id) before it reaches this table;
-- the plaintext is never stored, returned or logged. Nothing reads the ciphertext yet: the seal
-- and redeem routes that consume it ship later, behind their own switch.
--
-- Not a column of integration_connections: a binding is not a provider connection and must not
-- appear in connection listings, token leases or the connection export.
--
-- A revoked binding keeps its metadata row (the audit trail names it by id) but its ciphertext is
-- emptied by the revoke statement, and the CHECK below makes that a database fact.

CREATE TABLE IF NOT EXISTS credential_bindings (
  id TEXT PRIMARY KEY CHECK (id LIKE 'cb\_%'),
  organization_id TEXT NOT NULL CHECK (organization_id <> ''),
  created_by_user_id TEXT NOT NULL CHECK (created_by_user_id <> ''),
  kind TEXT NOT NULL CHECK (kind IN ('mcp_bearer', 'rest_api_key', 'rest_bearer')),
  -- ADR section 4 host grammar: lowercase ASCII, no trailing dot, no IP literal, no wildcard, no
  -- userinfo, no port (443 is the only port a binding is ever used on). The API validates the
  -- full grammar; this guards the table against a writer that skips it.
  host TEXT NOT NULL CHECK (host ~ '^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$'),
  header_name TEXT NOT NULL CHECK (header_name IN ('Authorization', 'X-Api-Key', 'Api-Key', 'X-Auth-Token')),
  allowed_methods TEXT[] NOT NULL DEFAULT '{}',
  allowed_path_prefixes TEXT[] NOT NULL DEFAULT '{}',
  secret_ciphertext BYTEA NOT NULL,
  key_version TEXT NOT NULL CHECK (key_version <> ''),
  status TEXT NOT NULL CHECK (status IN ('active', 'revoked')),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  revoked_at TIMESTAMPTZ,
  CHECK ((status = 'revoked') = (revoked_at IS NOT NULL)),
  CHECK (status = 'revoked' OR octet_length(secret_ciphertext) > 0),
  CHECK (status = 'active' OR octet_length(secret_ciphertext) = 0)
);

CREATE INDEX IF NOT EXISTS credential_bindings_org_idx
  ON credential_bindings (organization_id, created_at DESC);

CREATE INDEX IF NOT EXISTS credential_bindings_org_host_active_idx
  ON credential_bindings (organization_id, host)
  WHERE status = 'active';
