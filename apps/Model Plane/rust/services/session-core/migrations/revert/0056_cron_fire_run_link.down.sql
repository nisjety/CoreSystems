-- Reverts 0056_cron_fire_run_link.sql. Drops the fire-to-run link, the outcome and the
-- withheld-effect count. Fires keep their status and error, so the receipt-only path
-- loses nothing it wrote.

DROP INDEX IF EXISTS cron_fires_run_idx;
DROP INDEX IF EXISTS cron_fires_schedule_fire_key_idx;
ALTER TABLE cron_fires DROP COLUMN IF EXISTS outcome;
ALTER TABLE cron_fires DROP COLUMN IF EXISTS withheld_effects;
ALTER TABLE cron_fires DROP COLUMN IF EXISTS thread_id;
ALTER TABLE cron_fires DROP COLUMN IF EXISTS run_id;
ALTER TABLE cron_fires DROP COLUMN IF EXISTS fire_key;
