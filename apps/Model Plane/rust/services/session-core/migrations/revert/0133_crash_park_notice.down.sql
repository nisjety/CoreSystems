-- Reverses 0133_crash_park_notice.sql: drops the crash-park notice's once-only mark.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build without the crash-park notice first. A build that still stamps the mark
-- fails its recovery tick on the missing column. Dropping the column loses only the mark: a park that was already
-- notified is notified again only if the notice is switched back on while that park is still due.
--
-- ORDER:
--   1. run this file;
--   2. delete the 0133 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (the DROP is IF EXISTS).

BEGIN;
ALTER TABLE run_loop_checkpoints DROP COLUMN IF EXISTS park_notice_sent_at;
COMMIT;
