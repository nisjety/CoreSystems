-- Reverts 0074_approval_gate_context.sql.
--
-- Drops the gate-context columns. A standing-rule pass or a once-token that was recorded
-- in them is lost, so the reversal is safe only while `EXECUTION_CORE_SPACE_APPROVAL_GATE`
-- is `off` everywhere; the approvals themselves, their decisions and `consumed_at` (0046)
-- are untouched.
DROP INDEX IF EXISTS approvals_once_token_idx;
ALTER TABLE approvals
    DROP COLUMN IF EXISTS grant_ref,
    DROP COLUMN IF EXISTS turn_origin,
    DROP COLUMN IF EXISTS addressee_subject_id,
    DROP COLUMN IF EXISTS addressee_kind,
    DROP COLUMN IF EXISTS action_digest,
    DROP COLUMN IF EXISTS tool_key;
