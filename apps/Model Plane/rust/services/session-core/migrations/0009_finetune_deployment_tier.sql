-- 0009_finetune_deployment_tier.sql
--
-- Wave 7 follow-up — persist the hosting SKU tier of a fine-tuned model's
-- deployment.
--
--   * `developer`  — $0/hr, auto-deletes after 24h. Auto-deploys (poller) land
--                    here so a fine-tune candidate never silently accrues the
--                    hourly hosting charge.
--   * `production` — paid Standard hosting. Set only by an explicit operator
--                    promote (POST /v1/finetune/jobs/:job_id/deploy).
--
-- Defaults to 'developer' so existing rows (pre-migration) read as the safe,
-- free tier. Idempotent ADD COLUMN IF NOT EXISTS.

ALTER TABLE finetune_jobs
    ADD COLUMN IF NOT EXISTS deployment_tier TEXT NOT NULL DEFAULT 'developer';
