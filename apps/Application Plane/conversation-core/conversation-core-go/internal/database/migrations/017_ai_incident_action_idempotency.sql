-- A reviewed AI incident declaration may be redelivered. Tie its durable
-- effect to the AI-action identifier so retries return the original Incident
-- rather than creating a second operational event.
ALTER TABLE conversation_incidents
    ADD COLUMN IF NOT EXISTS ai_action_id TEXT NOT NULL DEFAULT '';

CREATE UNIQUE INDEX IF NOT EXISTS conversation_incidents_org_ai_action_key
    ON conversation_incidents (org_id, ai_action_id)
    WHERE ai_action_id <> '';
