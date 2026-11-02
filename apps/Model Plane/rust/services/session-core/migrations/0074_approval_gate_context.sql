-- Approval gate context (build plan WS10-S3).
--
-- A Space run's gated call now carries what Session Core needs to match a retry to the
-- approval that was granted for it, and to record that a standing rule (not a person)
-- passed the prompt:
--
--   tool_key              stable key of the gated tool (plain identifier)
--   action_digest         sha256 of the canonical {tool, input}; NULL under zero data
--                         retention and for any approval the gate did not raise
--   addressee_kind,       who may decide it; WS10-S6 writes them, S3 only adds the
--   addressee_subject_id  columns so one migration covers the slice group
--   turn_origin           'direct' | 'ambient' | 'automation'; NULL reads as 'direct'
--   grant_ref             the Control standing-rule id when a rule passed the prompt
--
-- The once-token (consumed_at) and continuation_mode already exist from 0046, so this
-- migration adds neither. Additive: every column is nullable or defaulted, so every
-- existing row and writer stays valid.
ALTER TABLE approvals
    ADD COLUMN IF NOT EXISTS tool_key TEXT,
    ADD COLUMN IF NOT EXISTS action_digest TEXT,
    ADD COLUMN IF NOT EXISTS addressee_kind TEXT NOT NULL DEFAULT 'requester'
        CHECK (addressee_kind IN ('requester', 'named', 'any_editor', 'any_manager')),
    ADD COLUMN IF NOT EXISTS addressee_subject_id TEXT,
    ADD COLUMN IF NOT EXISTS turn_origin TEXT
        CHECK (turn_origin IS NULL OR turn_origin IN ('direct', 'ambient', 'automation')),
    ADD COLUMN IF NOT EXISTS grant_ref TEXT;

-- The once-token lookup: a granted, unspent approval of this person for this exact call.
CREATE INDEX IF NOT EXISTS approvals_once_token_idx
    ON approvals (org_id, user_id, tool_key, action_digest)
    WHERE status = 'granted' AND consumed_at IS NULL AND action_digest IS NOT NULL;
