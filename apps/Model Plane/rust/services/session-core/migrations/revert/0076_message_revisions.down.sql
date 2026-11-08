-- Reverses 0076_message_revisions.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only,
-- inside one transaction.
--
-- Dropping the table un-hides every deleted message and shows every original again: under
-- `retain` the original text is still in messages.content and would reappear in the room, in
-- model context and in Dreaming. Under `purge` the original is already gone and a deleted
-- message would read as an empty turn. Do not run this while any revision exists unless that is
-- intended.
--
-- ORDER, all three steps:
--   1. roll session-core and model-gateway back to builds that do not read message_revisions;
--   2. confirm the count (SELECT count(*) FROM message_revisions) is acceptable to lose;
--   3. run this file. A build that does read message_revisions must be stopped before step 3,
--      or its reads fail with "relation message_revisions does not exist" until it restarts.
--
-- Safe to run twice (both statements are IF EXISTS).

BEGIN;
DROP INDEX IF EXISTS idx_message_revisions_thread;
DROP TABLE IF EXISTS message_revisions;
COMMIT;
