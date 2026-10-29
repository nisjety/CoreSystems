DROP INDEX IF EXISTS idx_cost_events_user_created;
ALTER TABLE cost_events DROP COLUMN IF EXISTS user_id;
