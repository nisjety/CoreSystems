-- Reverts 0053_attention_state.sql.

DROP INDEX IF EXISTS plans_attention_proposed_idx;
DROP INDEX IF EXISTS approvals_attention_pending_idx;
