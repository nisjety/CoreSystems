-- Durable send receipts (build plan WS04 S6).
--
-- A message the user sends had no identity the transcript could dedupe on: a retry
-- after a lost reply, or a mid-run message the run never took and the client then
-- sent again as a new turn, was stored twice. AppendMessage now takes an opaque
-- `client_message_id`; this column and its unique index are what make a repeat of the
-- same id land as ONE row.
--
--   client_message_id  the caller's id for the message. NULL for every message sent
--                      without one (all of them, until the gateway's
--                      MODEL_GATEWAY_SEND_RECEIPTS switch is on), so existing rows
--                      and callers are untouched. Opaque: never derived from content,
--                      and never written for a zero-retention turn (a zero-retention
--                      credential is refused by AppendMessage before any row exists).
--
-- Unique per (thread_id, client_message_id) where set: two sends with the same id
-- cannot both be stored, and the same id on another thread is another message.
-- AppendMessage looks the id up under the thread lock before it inserts, so the index
-- is the backstop, not the mechanism.
--
-- Additive: a nullable column, no default, no backfill. The CHECK is added NOT VALID
-- and validated second, as 0050 and 0017 do: a validated CHECK scans the table under
-- ACCESS EXCLUSIVE, and `messages` is hot. The reversal is
-- revert/0052_message_client_id.down.sql.
ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS client_message_id TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'messages_client_message_id_check'
    ) THEN
        ALTER TABLE messages
            ADD CONSTRAINT messages_client_message_id_check
            CHECK (
                client_message_id IS NULL
                OR client_message_id ~ '^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$'
            ) NOT VALID;
    END IF;
END
$$;
ALTER TABLE messages VALIDATE CONSTRAINT messages_client_message_id_check;

CREATE UNIQUE INDEX IF NOT EXISTS messages_thread_client_message_id_uq
    ON messages (thread_id, client_message_id)
    WHERE client_message_id IS NOT NULL;
