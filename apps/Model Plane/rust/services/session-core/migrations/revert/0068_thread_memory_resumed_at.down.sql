-- Reverses 0068_thread_memory_resumed_at.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only,
-- inside one transaction.
--
-- Removing the column forgets when each thread was last resumed. Messages written while
-- a thread was off then become eligible for extraction again once it is back on, which
-- is the behaviour before this watermark existed. Tell users first. MEMORY_CONTROLS_ENABLED
-- =false is a stop-gap only: the readers then ignore the column, and the stored choices
-- are kept for when the switch is turned back on.
--
-- ORDER, all three steps:
--   1. roll session-core back to a build that does not read memory_resumed_at;
--   2. run this file;
--   3. the older build never mentions the column, so no restart is needed for it.
--      A build that does read it must be stopped before step 2, or its extraction
--      reads fail with "column memory_resumed_at does not exist" until it restarts.
--
-- Safe to run twice (the statement is IF EXISTS).

BEGIN;
ALTER TABLE threads DROP COLUMN IF EXISTS memory_resumed_at;
COMMIT;
