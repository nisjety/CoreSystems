-- Bind durable provider-write receipts to signed, content-free authorization
-- identifiers. Rows created before this migration retain empty attestation
-- columns and cannot be created or replayed through the new write API.

ALTER TABLE integration_action_receipts
  ADD COLUMN IF NOT EXISTS attestation_issuer TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS attestation_kid TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS authorization_kind TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS authorization_id TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS approval_id TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS action_id TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS actor_id TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS attestation_jti TEXT NOT NULL DEFAULT '',
  ADD COLUMN IF NOT EXISTS payload_sha256 TEXT NOT NULL DEFAULT '';

ALTER TABLE integration_action_receipts
  DROP CONSTRAINT IF EXISTS integration_action_receipts_status_check;

ALTER TABLE integration_action_receipts
  ALTER COLUMN status SET DEFAULT 'pending',
  ADD CONSTRAINT integration_action_receipts_status_check
    CHECK (status IN ('pending', 'executing', 'completed', 'unknown')),
  ADD CONSTRAINT integration_action_receipts_attestation_shape_check
    CHECK (
      (
        attestation_issuer = '' AND attestation_kid = '' AND
        authorization_kind = '' AND authorization_id = '' AND
        approval_id = '' AND action_id = '' AND actor_id = '' AND
        attestation_jti = '' AND payload_sha256 = ''
      ) OR (
        attestation_issuer = 'conversation-core' AND
        attestation_kid <> '' AND authorization_id <> '' AND
        action_id <> '' AND actor_id <> '' AND attestation_jti <> '' AND
        payload_sha256 ~ '^[0-9a-f]{64}$' AND
        (
          (authorization_kind = 'human_intent' AND approval_id = '') OR
          (authorization_kind = 'human_approved_ai_action' AND approval_id <> '')
        )
      )
    );

CREATE UNIQUE INDEX IF NOT EXISTS integration_action_receipts_authorization_unique
  ON integration_action_receipts (attestation_issuer, organization_id, authorization_id)
  WHERE attestation_issuer <> '' AND authorization_id <> '';

CREATE INDEX IF NOT EXISTS integration_action_receipts_pending_idx
  ON integration_action_receipts (status, updated_at ASC)
  WHERE status = 'pending';
