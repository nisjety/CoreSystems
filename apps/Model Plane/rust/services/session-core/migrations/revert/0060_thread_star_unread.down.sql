-- Reverts 0060_thread_star_unread.sql.
--
-- Dropping the columns drops the stars and the read markers with them. The
-- pre-star binary reads none of these columns, and the list orders on
-- last_activity_at, so no thread changes position after a rollback.

ALTER TABLE threads
    DROP COLUMN IF EXISTS last_read_at,
    DROP COLUMN IF EXISTS last_assistant_at,
    DROP COLUMN IF EXISTS starred_at;
