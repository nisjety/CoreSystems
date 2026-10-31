-- Reverses 0067_thread_memory_mode.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- Dropping the column discards every thread's memory-off choice: from then on those
-- threads follow the user's memory state again, which for most users is `on`. Tell
-- users first, or set MEMORY_CONTROLS_ENABLED=false only as a stop-gap, because that
-- restores the same behaviour while keeping the choices for when it is turned back on.
--
-- ORDER, all three steps:
--   1. roll session-core back to a build that does not read memory_mode;
--   2. run this file;
--   3. the older build never mentions the column, so no restart is needed for it.
--      A build that does read it must be stopped before step 2, or its memory reads
--      and writes fail with "column memory_mode does not exist" until it restarts.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;
ALTER TABLE threads DROP CONSTRAINT IF EXISTS threads_memory_mode_check;
ALTER TABLE threads DROP COLUMN IF EXISTS memory_mode;
COMMIT;
