-- Add idempotency_key for job creation de-dupe. Verevon's onboarding
-- wizard generates a key per submit attempt; if the user double-clicks
-- the Submit button or the network retries the same POST, control
-- returns the existing job record (200) instead of creating a
-- duplicate (201). NULL is allowed so legacy/scheduler-generated jobs
-- (no key) keep working.
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS idempotency_key TEXT;
-- Unique only where present — multiple legacy NULLs are fine.
CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_idempotency_key
    ON jobs(idempotency_key) WHERE idempotency_key IS NOT NULL;
