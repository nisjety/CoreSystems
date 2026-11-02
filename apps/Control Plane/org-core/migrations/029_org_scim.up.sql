-- WS05-S8: SCIM Users. Bearer tokens an owner/admin mints for their identity
-- provider, and the SCIM user records those tokens manage.
--
-- Only identity keys are stored (user name = the normalised e-mail, external id,
-- the auth-core user id). displayName and every other profile attribute is
-- neither stored nor applied: profile data belongs to auth-core and user-core.
-- The membership itself lives in auth-core's member table (no status column);
-- organization_scim_users records what SCIM did, so a retry or a re-activation
-- can compare state instead of repeating a change.
--
-- Additive and idempotent.

CREATE TABLE IF NOT EXISTS organization_scim_tokens (
  id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  token_prefix TEXT NOT NULL,
  -- sha256 hex of the bearer token. Globally unique: the organisation of a SCIM
  -- request is derived from this row, never from a header.
  token_hash TEXT NOT NULL UNIQUE,
  created_by TEXT NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL,
  last_used_at TIMESTAMPTZ,
  revoked_at TIMESTAMPTZ,
  revoked_by TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT organization_scim_tokens_name_nonempty CHECK (btrim(name) <> '' AND length(name) <= 80),
  CONSTRAINT organization_scim_tokens_hash_hex CHECK (token_hash ~ '^[0-9a-f]{64}$')
);

CREATE INDEX IF NOT EXISTS idx_organization_scim_tokens_org
  ON organization_scim_tokens (org_id, created_at DESC);

CREATE TABLE IF NOT EXISTS organization_scim_users (
  org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  scim_id UUID NOT NULL,
  external_id TEXT,
  user_name TEXT NOT NULL,            -- normalised e-mail, the immutable identity key
  user_id TEXT,                       -- auth-core user id once provisioned
  active BOOLEAN NOT NULL,
  role_at_deactivation TEXT,
  deprovisioned_at TIMESTAMPTZ,
  deleted_at TIMESTAMPTZ,             -- DELETE keeps a tombstone
  revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (org_id, scim_id),
  UNIQUE (org_id, user_name),
  CONSTRAINT organization_scim_users_user_name_nonempty CHECK (btrim(user_name) <> '')
);

CREATE INDEX IF NOT EXISTS idx_organization_scim_users_external
  ON organization_scim_users (org_id, external_id);

-- Migration 009 default privileges grant all four verbs on every later table;
-- state the effective set explicitly (as 024 and 028 do). Tokens are never hard
-- deleted by the app role: revocation is a timestamp.
REVOKE ALL ON organization_scim_tokens FROM org_core_app;
GRANT SELECT, INSERT, UPDATE ON organization_scim_tokens TO org_core_app;

REVOKE ALL ON organization_scim_users FROM org_core_app;
GRANT SELECT, INSERT, UPDATE ON organization_scim_users TO org_core_app;

ALTER TABLE organization_scim_tokens ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_scim_tokens_rls_isolation ON organization_scim_tokens;
CREATE POLICY organization_scim_tokens_rls_isolation
  ON organization_scim_tokens
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE organization_scim_users ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_scim_users_rls_isolation ON organization_scim_users;
CREATE POLICY organization_scim_users_rls_isolation
  ON organization_scim_users
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
