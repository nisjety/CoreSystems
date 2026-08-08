ALTER TABLE approval_delivery_outbox
    ADD COLUMN IF NOT EXISTS settled_at TIMESTAMPTZ;

ALTER TABLE approval_delivery_outbox
    DROP CONSTRAINT IF EXISTS approval_delivery_outbox_state_check;

ALTER TABLE approval_delivery_outbox
    ADD CONSTRAINT approval_delivery_outbox_state_check
    CHECK (state IN ('pending', 'processing', 'terminal', 'settled'));

-- A settled internal delivery means the verified worker completed its
-- continuation protocol. It is expressly not a provider/customer delivery
-- assertion and carries no provider response body.
CREATE INDEX IF NOT EXISTS approval_delivery_outbox_settled_idx
    ON approval_delivery_outbox (settled_at DESC)
    WHERE state = 'settled';
