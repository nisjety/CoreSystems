-- Reverse of 20260805150000_orchestrator_job_leases.sql.
--
-- WARNING: this returns `data_orchestrator_jobs` to a queue with no lease, so it
-- must be paired with reverting the claim-based worker in data-orchestrator-go.
-- Dropping the columns while the poller is still deployed makes every claim
-- query fail.
--
-- Any job currently leased is returned to `pending` first, so no row is left in
-- `running` with lease state that is about to be deleted — the exact orphan this
-- migration existed to remove.

UPDATE data_orchestrator_jobs
SET status = 'pending', started_at = NULL, updated_at = NOW()
WHERE status = 'running';

DROP INDEX IF EXISTS idx_orchestrator_jobs_claim;

ALTER TABLE data_orchestrator_jobs
    DROP CONSTRAINT IF EXISTS data_orchestrator_jobs_attempts_check;

ALTER TABLE data_orchestrator_jobs
    DROP COLUMN IF EXISTS lease_until,
    DROP COLUMN IF EXISTS lease_owner,
    DROP COLUMN IF EXISTS attempts;
