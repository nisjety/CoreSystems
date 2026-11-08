ALTER TABLE jobs ADD COLUMN IF NOT EXISTS schedule_id TEXT;
CREATE INDEX IF NOT EXISTS idx_jobs_schedule_id_created_at
    ON jobs(schedule_id, created_at DESC, id DESC);
