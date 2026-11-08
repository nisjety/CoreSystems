-- Add the signed initiating actor to new browser-action receipts.
--
-- Existing evidence must not be guessed or backfilled: actor-scoped reads
-- deliberately exclude legacy rows with a NULL actor_id. This keeps a tenant
-- membership from becoming authority over another user's browser history.
ALTER TABLE quarry_step_receipts
    ADD COLUMN IF NOT EXISTS actor_id TEXT;

CREATE INDEX IF NOT EXISTS quarry_step_receipts_org_actor_run_idx
    ON quarry_step_receipts (org_id, actor_id, run_id, step ASC, finished_at ASC)
    WHERE actor_id IS NOT NULL;
