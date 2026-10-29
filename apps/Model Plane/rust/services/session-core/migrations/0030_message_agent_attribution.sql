-- 0030_message_agent_attribution — which persona an assistant turn answered
-- as, recorded at the time of the turn. Presentation history, not authority:
-- the invocation was authorized upstream, and renaming an agent later must
-- not rewrite what the room saw. NULL for user/system/tool messages and for
-- assistant turns that ran without a persona.

ALTER TABLE messages ADD COLUMN IF NOT EXISTS agent_name TEXT;
