-- Persisted message tree (build plan WS17 S8).
--
-- A turn can have more than one continuation: a regenerated answer, an edited turn,
-- a rewind. The tree keeps every branch and marks one chain as the active path, from
-- the first turn of the thread to its leaf. Nothing is deleted: a rewind moves the leaf.
--
--   parent_message_id              the turn this one continues; NULL for the first turn
--                                  of a thread. Written by the append path and by the
--                                  one-time link of a legacy chain, both only while
--                                  SESSION_CORE_MESSAGE_VERSIONS_ENABLED is on. No
--                                  foreign key: a missed link reads as a root, and an
--                                  insert is never refused by it.
--   sibling_ordinal                1-based place among the turns that share the parent.
--   on_active_path                 TRUE for the active chain. Every history reader filters
--                                  on it whether or not the switch is on, so a turn on an
--                                  inactive branch never reaches a prompt, learning or
--                                  Dreaming. Every existing row defaults to TRUE, so the
--                                  readers answer exactly as before.
--   threads.active_leaf_message_id the last turn of the active path, the compare-and-set
--                                  token of SetActiveLeaf. NULL until the thread is
--                                  materialised.
--
-- Additive: defaulted or nullable columns and no backfill. The migration never scans
-- a transcript; a legacy chain is linked by the first versioned write to its thread.
-- An older binary keeps working on the new columns: it inserts with the defaults, and
-- its turns read as active.
--
-- This file runs in one transaction, as sqlx sends a migration as a single statement
-- batch, and PostgreSQL refuses CREATE INDEX CONCURRENTLY inside a batch. The two
-- indexes are therefore plain: building them takes a SHARE lock on messages until the
-- build ends, so writers wait for it. Build them CONCURRENTLY out of band on a large
-- transcript before the migration runs there; the IF NOT EXISTS makes that a no-op.
-- The reversal is in revert/0061_message_versions.down.sql.

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS parent_message_id TEXT,
    ADD COLUMN IF NOT EXISTS sibling_ordinal INTEGER NOT NULL DEFAULT 1,
    ADD COLUMN IF NOT EXISTS on_active_path BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS active_leaf_message_id TEXT;

CREATE INDEX IF NOT EXISTS idx_messages_parent
    ON messages (thread_id, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_messages_thread_active
    ON messages (thread_id, sequence)
    WHERE on_active_path;
