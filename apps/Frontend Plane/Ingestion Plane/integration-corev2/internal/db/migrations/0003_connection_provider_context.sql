ALTER TABLE integration_connections
  ADD COLUMN IF NOT EXISTS provider_context JSONB NOT NULL DEFAULT '{}'::jsonb;
