-- Keyset paging for the chat list (build plan WS17 S1).
--
-- ListThreads ordered on the newest message through a lateral per thread. A
-- keyset cursor needs that value as a stored column, so it can be seeked on
-- without reading every thread's messages. append_message_in_tx writes the
-- column in the same transaction as the message, so it moves exactly when the
-- lateral would have. Existing threads are backfilled from their newest message,
-- or their creation time when they have none, which is the value the lateral
-- produced. The index covers the owner-bound active listing in its paging order.
-- The reversal is in revert/0049_thread_activity_cursor.down.sql.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS last_activity_at TIMESTAMPTZ NOT NULL DEFAULT now();

UPDATE threads AS t
   SET last_activity_at = COALESCE(
       (SELECT max(m.created_at) FROM messages AS m WHERE m.thread_id = t.id),
       t.created_at);

CREATE INDEX IF NOT EXISTS idx_threads_owner_activity
    ON threads (org_id, user_id, (pinned_at IS NOT NULL) DESC, last_activity_at DESC, id DESC)
    WHERE archived_at IS NULL;
