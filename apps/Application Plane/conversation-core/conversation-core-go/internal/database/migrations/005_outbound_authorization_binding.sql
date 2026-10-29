-- 005: bind every provider-write authorization to the exact durable effect.
--
-- These fields contain only identifiers, operation metadata, and a SHA-256
-- digest of the canonical provider request. Existing rows intentionally remain
-- empty and therefore cannot authorize a provider call in current code.

ALTER TABLE conversation_outbound_intents
    ADD COLUMN IF NOT EXISTS authorization_kind TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS actor_user_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS approval_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS action_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS operation TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS payload_sha256 TEXT NOT NULL DEFAULT '';

ALTER TABLE conversation_outbound_intents
    ADD CONSTRAINT conversation_outbound_intents_authorization_kind_check
        CHECK (authorization_kind IN ('', 'human_intent', 'human_approved_ai_action')),
    ADD CONSTRAINT conversation_outbound_intents_payload_sha256_check
        CHECK (payload_sha256 = '' OR payload_sha256 ~ '^[0-9a-f]{64}$');

ALTER TABLE conversation_outbound_intents
    DROP CONSTRAINT IF EXISTS conversation_outbound_intents_status_check;

ALTER TABLE conversation_outbound_intents
    ADD CONSTRAINT conversation_outbound_intents_status_check
        CHECK (status IN ('sending', 'retryable', 'submitted', 'failed', 'unknown'));
