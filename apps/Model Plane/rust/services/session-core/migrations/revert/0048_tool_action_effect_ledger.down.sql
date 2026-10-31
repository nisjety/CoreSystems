-- Reverts 0048_tool_action_effect_ledger.sql.
--
-- The two new statuses do not exist in the narrower constraint, so rows that hold
-- them are mapped to `failed` first. That loses the distinction between a call that
-- never ran and one whose outcome is unknown; both read as failed afterwards. The
-- class, digest, closer and receipt columns are dropped with their data.

UPDATE session_tool_audit_intents
   SET status = 'failed'
 WHERE status IN ('not_run', 'unknown_outcome');

ALTER TABLE session_tool_audit_intents
    DROP CONSTRAINT IF EXISTS session_tool_audit_intents_status_check;

ALTER TABLE session_tool_audit_intents
    ADD CONSTRAINT session_tool_audit_intents_status_check
        CHECK (status IN ('reserved', 'completed', 'failed')),
    DROP COLUMN IF EXISTS args_digest,
    DROP COLUMN IF EXISTS effect_class,
    DROP COLUMN IF EXISTS closed_by,
    DROP COLUMN IF EXISTS provider_receipt_id;
