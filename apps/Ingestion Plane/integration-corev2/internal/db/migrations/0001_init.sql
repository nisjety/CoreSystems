CREATE TABLE IF NOT EXISTS integration_oauth_sessions (
  id TEXT PRIMARY KEY,
  provider_key TEXT NOT NULL,
  connector_type TEXT NOT NULL,
  organization_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  user_email TEXT NOT NULL DEFAULT '',
  state_hash TEXT NOT NULL UNIQUE,
  code_verifier_ciphertext TEXT NOT NULL,
  redirect_uri TEXT NOT NULL,
  return_url TEXT NOT NULL DEFAULT '',
  provider_context JSONB NOT NULL DEFAULT '{}'::jsonb,
  capabilities TEXT[] NOT NULL DEFAULT '{}',
  scopes TEXT[] NOT NULL DEFAULT '{}',
  expires_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  consumed_at TIMESTAMPTZ,
  error_code TEXT NOT NULL DEFAULT '',
  error_description TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS integration_oauth_sessions_org_idx
  ON integration_oauth_sessions (organization_id, provider_key, created_at DESC);

CREATE TABLE IF NOT EXISTS integration_connections (
  id TEXT PRIMARY KEY,
  provider_key TEXT NOT NULL,
  connector_type TEXT NOT NULL,
  organization_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  user_email TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL,
  display_name TEXT NOT NULL DEFAULT '',
  provider_account_id TEXT NOT NULL DEFAULT '',
  tenant_id TEXT NOT NULL DEFAULT '',
  provider_context JSONB NOT NULL DEFAULT '{}'::jsonb,
  capabilities TEXT[] NOT NULL DEFAULT '{}',
  scopes TEXT[] NOT NULL DEFAULT '{}',
  encrypted_access_token TEXT NOT NULL,
  encrypted_refresh_token TEXT NOT NULL DEFAULT '',
  access_token_expires_at TIMESTAMPTZ NOT NULL,
  last_refreshed_at TIMESTAMPTZ,
  last_sync_status TEXT NOT NULL DEFAULT '',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS integration_connections_org_connector_idx
  ON integration_connections (organization_id, connector_type, deleted_at);

CREATE TABLE IF NOT EXISTS integration_audit_events (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  user_id TEXT NOT NULL DEFAULT '',
  connection_id TEXT NOT NULL DEFAULT '',
  event_type TEXT NOT NULL,
  provider_key TEXT NOT NULL DEFAULT '',
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS integration_audit_events_org_idx
  ON integration_audit_events (organization_id, created_at DESC);
