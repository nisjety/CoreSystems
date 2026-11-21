-- Reverses 0134_messages_withdrawn_at.sql: drops the withdrawn mark from messages.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build without message withdrawal first. A build that still reads or stamps
-- the column fails its transcript reads on the missing column. Dropping the column loses only the mark: a message
-- that was withdrawn keeps its cleared text and stays off the active path, so it does not come back, but it is no
-- longer shown as a "retracted" note.
--
-- ORDER:
--   1. run this file;
--   2. delete the 0134 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (the DROP is IF EXISTS).

BEGIN;
ALTER TABLE messages DROP COLUMN IF EXISTS withdrawn_at;
COMMIT;
