-- 0035_message_author_attribution — which subject wrote a user turn.
--
-- Until now the only author a transcript could name was the thread's owner,
-- because `threads.user_id` was the only identity on the read path. That is
-- correct for a personal thread and wrong for a room: once several people post
-- in one Space, "whose words are these" has no answer in the data, and a
-- reader is left either guessing or labelling everything with whoever happens
-- to be looking.
--
-- Recorded at append time from the already-verified caller, never from a
-- client-supplied field. NULL for rows written before this column existed and
-- for assistant/system/tool turns, which are attributed by `agent_name`
-- instead. A NULL here must render as an unnamed author, never as the reader.

ALTER TABLE messages ADD COLUMN IF NOT EXISTS author_subject_id TEXT;

-- Reading a room means grouping a Space's turns by author for presentation;
-- the index keeps that from degrading into a per-thread scan as a room grows.
CREATE INDEX IF NOT EXISTS idx_messages_author_subject
    ON messages (thread_id, author_subject_id)
    WHERE author_subject_id IS NOT NULL;
