-- Reverses 0051_user_memory_state.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- Dropping the columns discards every user's pause/off choice: from then on every
-- user's memory is used and learned from again. Tell users first, or set
-- MEMORY_CONTROLS_ENABLED=false only as a stop-gap, because that restores the same
-- behaviour while keeping the choices for when it is turned back on. The change log
-- is dropped with the table.
--
-- ORDER, all three steps:
--   1. roll session-core back to a build that does not read memory_state;
--   2. run this file;
--   3. the older build never mentions the columns, so no restart is needed for it.
--      A build that does read them must be stopped before step 2, or its resolve
--      fails with "column memory_state does not exist" and every memory read and
--      write is refused until it restarts.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;
DROP TABLE IF EXISTS memory_control_events;
ALTER TABLE user_memory_controls DROP CONSTRAINT IF EXISTS user_memory_controls_state_check;
ALTER TABLE user_memory_controls
    DROP COLUMN IF EXISTS state_changed_by,
    DROP COLUMN IF EXISTS state_changed_at,
    DROP COLUMN IF EXISTS memory_state;
COMMIT;
