-- A proposal group is an immutable, opaque correlation key for independently
-- reviewed AI actions produced from one bounded resolution plan. It never
-- changes execution semantics: every member still has its own review and
-- execution receipt.
ALTER TABLE conversation_ai_actions
    ADD COLUMN IF NOT EXISTS proposal_group_id TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_conversation_ai_actions_org_proposal_group
    ON conversation_ai_actions (org_id, proposal_group_id, created_at DESC)
    WHERE proposal_group_id <> '';
