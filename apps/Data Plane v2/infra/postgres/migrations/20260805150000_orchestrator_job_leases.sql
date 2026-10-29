-- P2-4 — durable claiming for `data_orchestrator_jobs`.
--
-- DEFECT (plan D12): `data_orchestrator_jobs` was the one place in this plane
-- where the established lease/claim idiom was missing. `OrchestratorHandler.launch`
-- fired a bare `go func(){ executor.Run(...) }()` straight from the HTTP request
-- path. Nothing polled the table, so:
--   * a process restart between INSERT and completion orphaned the row in
--     `running` forever — no worker would ever pick it up again;
--   * work was pinned to whichever replica served the POST, so it could not be
--     retried elsewhere or spread across replicas;
--   * a transient failure was terminal, because `Run` returning an error only
--     logged.
--
-- The same table already had `status` and `idempotency_key`; what it lacked was
-- the three columns the rest of the plane uses to make a queue safe under
-- concurrency. Convention is deliberately identical to `wiki_event_outbox`,
-- `index_deletion_outbox` and `quickwit_admin_jobs`
-- (`attempts` / `lease_owner` / `lease_until`) so all four read the same way.
--
-- `attempts` is NOT NULL DEFAULT 0 so pre-existing rows are valid immediately.
-- The nullable lease columns mean "not currently claimed", which is the correct
-- state for every historical row.

ALTER TABLE data_orchestrator_jobs
    ADD COLUMN IF NOT EXISTS attempts INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS lease_owner TEXT,
    ADD COLUMN IF NOT EXISTS lease_until TIMESTAMPTZ;

ALTER TABLE data_orchestrator_jobs
    DROP CONSTRAINT IF EXISTS data_orchestrator_jobs_attempts_check;
ALTER TABLE data_orchestrator_jobs
    ADD CONSTRAINT data_orchestrator_jobs_attempts_check CHECK (attempts >= 0);

-- Partial index matching the claim predicate exactly: rows that are claimable
-- now (`pending`) or whose lease has expired (`running` past `lease_until`,
-- i.e. a crashed worker). Terminal rows are excluded from the index entirely,
-- so it stays small no matter how much history accumulates.
CREATE INDEX IF NOT EXISTS idx_orchestrator_jobs_claim
    ON data_orchestrator_jobs (created_at)
    WHERE status IN ('pending', 'running');

-- Recover rows orphaned by the previous fire-and-forget design. A `running` row
-- with no lease cannot be claimed (the predicate requires an expired lease), so
-- without this it would sit unreachable forever. Returning it to `pending` makes
-- it eligible; `completed_at` is already NULL for these, which the existing
-- `data_orchestrator_jobs_terminal_shape_check` requires for a non-terminal row.
UPDATE data_orchestrator_jobs
SET status = 'pending', started_at = NULL, updated_at = NOW()
WHERE status = 'running' AND lease_owner IS NULL;
