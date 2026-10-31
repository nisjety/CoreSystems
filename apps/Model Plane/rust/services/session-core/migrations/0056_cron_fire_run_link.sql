-- Build plan WS20 S1. A scheduled fire learns its fire key, the run and thread it
-- started, its truthful terminal status and how many effects were withheld.
-- Additive only: every new column has a default, so a fire written before this
-- migration keeps an empty fire_key, matches no run, and stays as it is (WS20 S8
-- sweeps it to unknown). The writes are gated by SESSION_CORE_SCHEDULED_RESULT_PERSIST.

ALTER TABLE cron_fires ADD COLUMN IF NOT EXISTS fire_key TEXT NOT NULL DEFAULT '';
ALTER TABLE cron_fires ADD COLUMN IF NOT EXISTS run_id TEXT NOT NULL DEFAULT '';
ALTER TABLE cron_fires ADD COLUMN IF NOT EXISTS thread_id TEXT NOT NULL DEFAULT '';
ALTER TABLE cron_fires ADD COLUMN IF NOT EXISTS withheld_effects INTEGER NOT NULL DEFAULT 0;
ALTER TABLE cron_fires ADD COLUMN IF NOT EXISTS outcome TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS cron_fires_schedule_fire_key_idx
    ON cron_fires (schedule_id, fire_key) WHERE fire_key <> '';
CREATE INDEX IF NOT EXISTS cron_fires_run_idx
    ON cron_fires (run_id) WHERE run_id <> '';
