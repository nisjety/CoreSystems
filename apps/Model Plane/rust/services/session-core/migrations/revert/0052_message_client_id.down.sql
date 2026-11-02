-- Reverses 0052_message_client_id.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction. Dropping the column discards every message's
-- recorded client id: a retry of a send stored before the drop is no longer
-- recognised, so turn MODEL_GATEWAY_SEND_RECEIPTS off on every gateway replica first
-- (they then send no id and persist-after-enqueue as before).
--
-- ORDER, both steps:
--   1. turn MODEL_GATEWAY_SEND_RECEIPTS off on every gateway replica;
--   2. run this file.
-- Session-core needs no restart: it touches the column only for an append that
-- carries a client_message_id, and an append that carries one while the column is
-- absent fails with a database error (an INTERNAL status) rather than writing a
-- message without its id.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;
DROP INDEX IF EXISTS messages_thread_client_message_id_uq;
ALTER TABLE messages DROP CONSTRAINT IF EXISTS messages_client_message_id_check;
ALTER TABLE messages DROP COLUMN IF EXISTS client_message_id;
COMMIT;
