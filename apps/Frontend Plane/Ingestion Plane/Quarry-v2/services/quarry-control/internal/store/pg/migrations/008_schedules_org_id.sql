-- W2 recurring change-monitoring: every schedule is tenant-scoped.
-- org_id rides into the Temporal workflow Args so baselines/diffs/notifications
-- stay org-scoped. Backfill is '' (empty) for any pre-existing single-tenant
-- rows; the edge stamps a verified org_id on every new schedule.
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS org_id TEXT NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS idx_schedules_org_id ON schedules(org_id);
