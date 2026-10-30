-- Per-thread memory off (build plan WS07 S6, optional).
--
-- The user's memory state (0051) is one choice for every thread. This is the
-- narrower one: a single conversation that should neither recall nor learn.
-- `memory_control::resolve_for_thread` folds it into the same decision the user
-- state goes through, so every memory reader and writer that names a thread sees
-- one answer:
--
--   inherit  the default: the thread follows the user's memory state
--   off      memory is not used and nothing is learned from this thread, whatever the
--            user's state; other threads are unaffected
--
-- The column holds a choice, never content. Every existing thread reads 'inherit',
-- so nothing changes for anyone until a thread is set. It is erased with the thread
-- (the column lives on the row).
--
-- Additive: a defaulted column (a catalog-only change on PG 11+), no backfill. The
-- reversal is in revert/0067_thread_memory_mode.down.sql.
ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS memory_mode TEXT NOT NULL DEFAULT 'inherit';

-- NOT VALID first, VALIDATE second, as 0050 and 0051 do: a validated CHECK scans the
-- table under ACCESS EXCLUSIVE, and `threads` is hot. The guard keeps a re-run harmless.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'threads_memory_mode_check'
    ) THEN
        ALTER TABLE threads
            ADD CONSTRAINT threads_memory_mode_check
            CHECK (memory_mode IN ('inherit', 'off')) NOT VALID;
    END IF;
END
$$;
ALTER TABLE threads VALIDATE CONSTRAINT threads_memory_mode_check;
