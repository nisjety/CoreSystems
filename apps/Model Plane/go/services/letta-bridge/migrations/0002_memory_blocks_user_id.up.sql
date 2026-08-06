-- Adds per-user ownership tracking to letta_memory_blocks so Delete (and,
-- later, List) can be safely scoped by user. Before this column existed,
-- DeleteMemory against this tier was an unconditional no-op: it could not
-- verify a memory_id belonged to the requesting user without either
-- ignoring ownership (a cross-user leak) or scanning everything (an
-- org-wide leak), so it declined to delete at all -- a DSAR/erasure
-- correctness gap. Existing rows predate per-user tracking and get the
-- empty-string default; they remain undeletable by user scope until
-- reindexed, which is the same limitation as before this migration, not a
-- new one.
ALTER TABLE letta_memory_blocks ADD COLUMN IF NOT EXISTS user_id TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_letta_memory_org_user
    ON letta_memory_blocks (org_id, user_id);
