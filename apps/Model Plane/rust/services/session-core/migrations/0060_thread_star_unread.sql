-- Starred and unread state for the chat list (build plan WS17 S7).
--
-- One unread definition, shared with WS16 (DEC-11): a thread is unread when its
-- newest assistant message is later than the owner's last read of it. A thread
-- with no assistant message keeps last_assistant_at NULL and is never unread.
-- last_read_at defaults to this migration's own timestamp, so every existing
-- thread reads as read: its backfilled last_assistant_at is older than that.
-- append_message_in_tx and the scheduled-result writer set last_assistant_at in
-- the same transaction as the message. The reversal is in
-- revert/0060_thread_star_unread.down.sql.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS starred_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_assistant_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_read_at TIMESTAMPTZ NOT NULL DEFAULT now();

UPDATE threads AS t
   SET last_assistant_at = latest.created_at
  FROM (
       SELECT thread_id, max(created_at) AS created_at
         FROM messages
        WHERE role = 'assistant'
        GROUP BY thread_id
       ) AS latest
 WHERE latest.thread_id = t.id;
