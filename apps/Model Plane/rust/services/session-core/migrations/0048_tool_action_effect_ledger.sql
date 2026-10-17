-- Effect ledger for tool calls (build plan WS03 S5).
--
-- session_tool_audit_intents already records a reservation before a call starts
-- and a result after it ends. A loop that dies between the two leaves the row
-- `reserved`, and nothing could say whether the effect happened. This widens the
-- row so the loop that resumes the run can close the call honestly:
--
--   not_run          closed without running: declined, expired, or ended first.
--   unknown_outcome  reserved, then the loop went away: the effect may or may not
--                    have happened, and the call is never retried.
--
-- A row carries the tool, its effect class and a digest of the arguments only,
-- never the arguments. Existing rows keep NULL in the new columns. The reversal is
-- in revert/0048_tool_action_effect_ledger.down.sql.

ALTER TABLE session_tool_audit_intents
    DROP CONSTRAINT IF EXISTS session_tool_audit_intents_status_check;

ALTER TABLE session_tool_audit_intents
    ADD CONSTRAINT session_tool_audit_intents_status_check
        CHECK (status IN ('reserved', 'completed', 'failed', 'not_run', 'unknown_outcome')),
    -- blake3 over the canonical JSON of the call's arguments.
    ADD COLUMN IF NOT EXISTS args_digest TEXT
        CHECK (args_digest IS NULL OR length(args_digest) = 64),
    -- What the call may change: none, read, write or external. NULL means the
    -- writer did not classify it (the gateway's read-only loop).
    ADD COLUMN IF NOT EXISTS effect_class TEXT
        CHECK (effect_class IS NULL OR length(effect_class) BETWEEN 1 AND 32),
    -- Who closed the row: the executor that ran the call, or the sweep that found
    -- it reserved long after its run ended.
    ADD COLUMN IF NOT EXISTS closed_by TEXT
        CHECK (closed_by IS NULL OR closed_by IN ('executor', 'sweep')),
    -- The provider's own receipt for a completed call, when it returns one.
    ADD COLUMN IF NOT EXISTS provider_receipt_id TEXT
        CHECK (provider_receipt_id IS NULL OR length(provider_receipt_id) BETWEEN 1 AND 256);
