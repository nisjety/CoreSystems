-- The Space kind of a thread, as Control signed it (build plan WS17-S1).
--
-- Memory containment (src/space/memory_scope.rs) treats a thread as contained when
-- space_id IS NOT NULL AND space_kind IS DISTINCT FROM 'personal'. A NULL kind therefore
-- fails closed: every thread created before the claim existed reads as contained until a
-- verified Control decision stamps it (see stamp_space_kind in grpc.rs).
--
-- No value list in the CHECK: Control owns the kind enum (it adds dm and group_dm in its own
-- migration), and an unknown kind is contained, which is the safe reading. No backfill.
-- Additive and nullable: a catalog-only change. Reversal: revert/0073_thread_space_kind.down.sql.
ALTER TABLE threads ADD COLUMN IF NOT EXISTS space_kind TEXT
    CHECK (space_kind IS NULL OR length(space_kind) BETWEEN 1 AND 32);
