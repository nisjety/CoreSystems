-- no-transaction
-- Reverses 0061_message_versions.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only
-- the top level of migrations/. Run it by hand, against the database only. CONCURRENTLY,
-- so it runs outside a transaction, as revert/0055 does.
--
-- ORDER, all three steps:
--   1. turn SESSION_CORE_MESSAGE_VERSIONS_ENABLED off on every session-core replica;
--   2. roll session-core back to the binary that predates the message tree;
--   3. run this file.
-- The current binary filters every history read on on_active_path, so it cannot serve
-- once the column is gone; step 2 must come first.
--
-- Dropping on_active_path makes every turn on an inactive branch an ordinary turn again:
-- the transcript and the prompts read every branch. Nothing is deleted, but a thread that
-- must keep its inactive branches hidden has to be rewound to its newest path
-- (SetActiveLeaf) before this runs.
--
-- Safe to run twice (every statement is IF EXISTS).

DROP INDEX CONCURRENTLY IF EXISTS idx_messages_thread_active;
DROP INDEX CONCURRENTLY IF EXISTS idx_messages_parent;

ALTER TABLE threads
    DROP COLUMN IF EXISTS active_leaf_message_id;

ALTER TABLE messages
    DROP COLUMN IF EXISTS on_active_path,
    DROP COLUMN IF EXISTS sibling_ordinal,
    DROP COLUMN IF EXISTS parent_message_id;
