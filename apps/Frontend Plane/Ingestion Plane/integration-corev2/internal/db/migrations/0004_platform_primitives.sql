CREATE TABLE IF NOT EXISTS integration_connection_consents (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  connection_id TEXT NOT NULL REFERENCES integration_connections(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL DEFAULT '',
  provider_key TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL,
  purpose TEXT NOT NULL,
  granted BOOLEAN NOT NULL DEFAULT true,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  expires_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  revoked_at TIMESTAMPTZ,
  UNIQUE (connection_id, source, purpose)
);

CREATE INDEX IF NOT EXISTS integration_connection_consents_connection_idx
  ON integration_connection_consents (connection_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS integration_sync_jobs (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  connection_id TEXT NOT NULL REFERENCES integration_connections(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL DEFAULT '',
  provider_key TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL,
  reason TEXT NOT NULL DEFAULT '',
  mode TEXT NOT NULL DEFAULT '',
  checkpoint JSONB NOT NULL DEFAULT '{}'::jsonb,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  started_at TIMESTAMPTZ,
  completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS integration_sync_jobs_org_idx
  ON integration_sync_jobs (organization_id, created_at DESC);

CREATE INDEX IF NOT EXISTS integration_sync_jobs_connection_idx
  ON integration_sync_jobs (connection_id, created_at DESC);

CREATE TABLE IF NOT EXISTS integration_sync_events (
  id TEXT PRIMARY KEY,
  job_id TEXT NOT NULL REFERENCES integration_sync_jobs(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  message TEXT NOT NULL DEFAULT '',
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS integration_sync_events_job_idx
  ON integration_sync_events (job_id, created_at ASC);

CREATE TABLE IF NOT EXISTS integration_webhook_events (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL DEFAULT '',
  provider_key TEXT NOT NULL,
  event_type TEXT NOT NULL,
  signature_hash TEXT NOT NULL DEFAULT '',
  payload JSONB NOT NULL DEFAULT '{}'::jsonb,
  received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- v1/Nango-era installations already have integration_webhook_events with
-- source/operation/nango_connection_id columns. Keep those columns for audit
-- history, but add the first-party v2 columns in place so cutover does not
-- require dropping the table or losing webhook records.
ALTER TABLE integration_webhook_events
  ADD COLUMN IF NOT EXISTS organization_id TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS provider_key TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS signature_hash TEXT NOT NULL DEFAULT '';

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = 'public'
      AND table_name = 'integration_webhook_events'
      AND column_name = 'source'
  ) THEN
    UPDATE integration_webhook_events
    SET provider_key = source
    WHERE provider_key = '' AND source IS NOT NULL;
  END IF;
END $$;

CREATE INDEX IF NOT EXISTS integration_webhook_events_provider_idx
  ON integration_webhook_events (provider_key, received_at DESC);

CREATE TABLE IF NOT EXISTS integration_token_leases (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  connection_id TEXT NOT NULL REFERENCES integration_connections(id) ON DELETE CASCADE,
  user_id TEXT NOT NULL DEFAULT '',
  provider_key TEXT NOT NULL,
  connector_type TEXT NOT NULL,
  consumer TEXT NOT NULL DEFAULT '',
  expires_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS integration_token_leases_connection_idx
  ON integration_token_leases (connection_id, created_at DESC);
