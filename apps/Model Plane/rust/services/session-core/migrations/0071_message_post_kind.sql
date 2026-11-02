-- Message kind for human posts in a Space room (build plan WS01-S1).
--
-- A 'turn' is what every row has been until now: the owner's prompt to the agent, the
-- agent's answer, a system or tool row. A 'post' is a human message written to the room
-- that is never sent to the agent. Posts are kept out of model context, agent memory and
-- Dreaming by the readers in session-core and model-gateway, not by this column alone.
--
-- Additive: a NOT NULL column with a constant default is a catalog-only change on current
-- Postgres, so no row is rewritten and every existing row reads 'turn'. The CHECK is added
-- NOT VALID and then validated, so the validation scan does not hold the exclusive lock the
-- ADD COLUMN needed. The reversal is in revert/0071_message_post_kind.down.sql.
ALTER TABLE messages ADD COLUMN IF NOT EXISTS post_kind TEXT NOT NULL DEFAULT 'turn';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'messages_post_kind_check' AND conrelid = 'messages'::regclass
    ) THEN
        ALTER TABLE messages
            ADD CONSTRAINT messages_post_kind_check CHECK (post_kind IN ('turn', 'post')) NOT VALID;
    END IF;
END
$$;

ALTER TABLE messages VALIDATE CONSTRAINT messages_post_kind_check;
