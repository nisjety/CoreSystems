-- Durable plan grant (build plan WS04 S4).
--
-- Approving a plan leaves plan mode at a named autonomy rung. That grant used to
-- live only in the memory of the gateway replica that took the approval, so a run
-- started on another replica, or after a restart, carried no rung. It is now a
-- fact about the THREAD (the follow-up run is a new run on the same thread), and
-- SetRunMode writes it in the same transaction as the run update when the approval
-- names its plan.
--
--   granted_rung           the rung label the approval granted: read_only |
--                          workspace_write | danger_full_access
--   granted_justification  the reason the approver stated (already kept on the run's
--                          metadata; copied so the grant reads back on its own)
--   granted_plan_id        the plan-gate plan approved with the grant, '' when none.
--                          Reading it does not consume it: the plan's own state
--                          (APPROVED -> EXECUTING, a compare-and-set) is the
--                          single-use guard
--   granted_at             when the approval was recorded
--   granted_expires_at     when the grant lapses (24 hours after granted_at); a read
--                          after this instant answers "no grant"
--
-- A thread that never held a grant keeps every column NULL. The grant is erased with
-- the thread (the columns live on the row). A zero-retention credential is refused
-- before this is written (the session:write gate), and so is a waived one that states a
-- grant (durable_mutation_forbidden), so no such thread ever carries one.
--
-- Additive: nullable columns, no default, no backfill. The reversal is in
-- revert/0050_thread_autonomy_grant.down.sql: it drops the columns and session-core must
-- be RESTARTED afterwards, because it checks once at boot whether the columns exist and,
-- with them absent, skips the thread's copy of a grant (and answers "no grant") instead
-- of failing.
ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS granted_rung TEXT,
    ADD COLUMN IF NOT EXISTS granted_justification TEXT,
    ADD COLUMN IF NOT EXISTS granted_plan_id TEXT,
    ADD COLUMN IF NOT EXISTS granted_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS granted_expires_at TIMESTAMPTZ;

-- NOT VALID first, VALIDATE second, as 0017 does: a validated CHECK scans the table
-- under ACCESS EXCLUSIVE, and `threads` is hot. The guard keeps a re-run harmless.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'threads_granted_rung_check'
    ) THEN
        ALTER TABLE threads
            ADD CONSTRAINT threads_granted_rung_check
            CHECK (
                granted_rung IS NULL
                OR granted_rung IN ('read_only', 'workspace_write', 'danger_full_access')
            ) NOT VALID;
    END IF;
END
$$;
ALTER TABLE threads VALIDATE CONSTRAINT threads_granted_rung_check;
