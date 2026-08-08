-- 011: provider-originated delivery evidence for already accepted outbound sends.
--
-- `status = submitted` continues to mean only that a provider accepted our
-- send request. These columns hold later provider callback evidence and are
-- deliberately metadata-only: no transcript content, recipient address, or
-- provider thread identifier is duplicated here.

ALTER TABLE conversation_outbound_intents
    ADD COLUMN IF NOT EXISTS provider_delivery_status TEXT NOT NULL DEFAULT 'unconfirmed',
    ADD COLUMN IF NOT EXISTS provider_delivery_occurred_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS provider_delivery_error_code TEXT NOT NULL DEFAULT '';

ALTER TABLE conversation_outbound_intents
    ADD CONSTRAINT conversation_outbound_intents_provider_delivery_status_check
    CHECK (provider_delivery_status IN ('unconfirmed', 'delivered', 'read', 'failed'));

CREATE INDEX IF NOT EXISTS conversation_outbound_intents_provider_receipt_lookup_idx
    ON conversation_outbound_intents (org_id, provider, provider_message_id)
    WHERE status = 'submitted' AND provider_message_id <> '';
