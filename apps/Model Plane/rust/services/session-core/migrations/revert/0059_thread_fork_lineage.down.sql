-- Reverts 0059_thread_fork_lineage.sql.
--
-- Dropping the columns drops the lineage with them. A fork then reads as an ordinary thread, and
-- its copied turns as ordinary turns, so Dreaming would extract from them again. Run it only
-- after SESSION_CORE_THREAD_FORK_ENABLED is off on every session-core replica, so no fork is
-- written while its lineage column is gone, and accept that existing forks are then re-read.
--
-- Safe to run twice: every statement is IF EXISTS.

DROP INDEX IF EXISTS idx_threads_forked_from;

ALTER TABLE messages
    DROP COLUMN IF EXISTS copied_from_message_id;

ALTER TABLE threads
    DROP COLUMN IF EXISTS forked_from_message_id,
    DROP COLUMN IF EXISTS forked_from_thread_id;
