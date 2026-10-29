-- Durable, content-free idempotency receipts for provider write actions.
--
-- A receipt is inserted before the provider call. A duplicate completed key
-- replays only the provider message identifier; executing/unknown receipts are
-- never called again blindly because the provider may already have accepted
-- the original request. Request bodies and provider response payloads are not
-- stored here (ZDR/PII boundary).

CREATE TABLE IF NOT EXISTS integration_action_receipts (
  organization_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  connection_id TEXT NOT NULL REFERENCES integration_connections(id) ON DELETE CASCADE,
  provider_key TEXT NOT NULL,
  operation TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'executing',
  provider_message_id TEXT NOT NULL DEFAULT '',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (organization_id, idempotency_key),
  CONSTRAINT integration_action_receipts_status_check
    CHECK (status IN ('executing', 'completed', 'unknown'))
);

CREATE INDEX IF NOT EXISTS integration_action_receipts_connection_idx
  ON integration_action_receipts (connection_id, created_at DESC);

CREATE INDEX IF NOT EXISTS integration_action_receipts_status_idx
  ON integration_action_receipts (status, updated_at ASC);
