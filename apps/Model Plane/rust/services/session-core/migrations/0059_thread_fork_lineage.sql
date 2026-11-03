-- Thread fork lineage (build plan WS17 S6).
--
-- A fork is a new thread that starts with a copy of another thread's active path, up to a
-- chosen assistant message. These columns record where the copy came from.
--
--   threads.forked_from_thread_id   the thread the copy was taken from. NULL for every thread
--                                   that was not forked.
--   threads.forked_from_message_id  the assistant message the copy ends with.
--   messages.copied_from_message_id the turn of the source thread this turn was copied from.
--                                   NULL for every turn written in its own thread. Dreaming never
--                                   extracts from a copied turn, because the source already did.
--
-- No foreign key: erasing a source leaves its forks whole, and erasing a fork leaves its source
-- untouched. Additive: nullable columns and no backfill, so every existing row reads as before.
-- The reversal is in revert/0059_thread_fork_lineage.down.sql.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS forked_from_thread_id TEXT,
    ADD COLUMN IF NOT EXISTS forked_from_message_id TEXT;

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS copied_from_message_id TEXT;

-- Plain rather than CONCURRENTLY, as 0061 is: the column is new and empty, so the build scans
-- an empty index and holds its lock only briefly.
CREATE INDEX IF NOT EXISTS idx_threads_forked_from
    ON threads (org_id, forked_from_thread_id)
    WHERE forked_from_thread_id IS NOT NULL;
