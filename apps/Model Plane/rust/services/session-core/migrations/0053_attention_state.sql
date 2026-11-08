-- Attention state (build plan WS16 S1).
--
-- The attention derivation (`attention.rs`) reads the pending rows of a list of threads: approvals
-- still requested, and plans still proposed. These partial indexes cover only those rows, so the
-- derivation never scans settled history.
--
-- No column is added. The unseen marker is `threads.last_read_at` (WS17 S7, DEC-11): a second seen
-- marker would be a second definition of unread. The reversal is in
-- revert/0053_attention_state.down.sql.

CREATE INDEX IF NOT EXISTS approvals_attention_pending_idx
    ON approvals (org_id, run_id)
    WHERE status = 'requested';

CREATE INDEX IF NOT EXISTS plans_attention_proposed_idx
    ON plans (org_id, thread_id)
    WHERE status = 'proposed';
