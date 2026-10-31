-- Reverts 0049_thread_activity_cursor.sql.
--
-- Dropping the column drops its values with it. The list falls back to the
-- lateral ordering that the pre-cursor binary uses, which reads the same
-- messages, so no thread changes position after a rollback.

DROP INDEX IF EXISTS idx_threads_owner_activity;

ALTER TABLE threads
    DROP COLUMN IF EXISTS last_activity_at;
