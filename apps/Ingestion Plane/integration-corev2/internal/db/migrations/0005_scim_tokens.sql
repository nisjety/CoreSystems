CREATE TABLE IF NOT EXISTS integration_scim_tokens (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '',
  token_prefix TEXT NOT NULL DEFAULT '',
  token_hash TEXT NOT NULL,
  created_by TEXT NOT NULL DEFAULT '',
  last_used_at TIMESTAMPTZ,
  expires_at TIMESTAMPTZ,
  revoked_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS integration_scim_tokens_hash_idx
  ON integration_scim_tokens (organization_id, token_hash)
  WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS integration_scim_tokens_org_idx
  ON integration_scim_tokens (organization_id, created_at DESC);
