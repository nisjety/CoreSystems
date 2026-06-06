ALTER TABLE integration_oauth_sessions
  ADD COLUMN IF NOT EXISTS provider_context JSONB NOT NULL DEFAULT '{}'::jsonb;
