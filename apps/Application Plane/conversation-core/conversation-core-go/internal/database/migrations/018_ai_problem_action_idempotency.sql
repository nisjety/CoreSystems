-- A reviewed AI Problem proposal must be retry-safe just like an approved
-- Incident declaration. Empty preserves ordinary human-created problems.
ALTER TABLE conversation_problems
    ADD COLUMN IF NOT EXISTS ai_action_id TEXT NOT NULL DEFAULT '';

CREATE UNIQUE INDEX IF NOT EXISTS conversation_problems_org_ai_action_unique
    ON conversation_problems (org_id, ai_action_id)
    WHERE ai_action_id <> '';
