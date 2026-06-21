-- W2: the change-monitor in-product notification reaches the schedule's
-- creator. created_by carries the user_id stamped from the edge's verified
-- JWT at create time. Empty for legacy / non-user-initiated schedules.
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS created_by TEXT NOT NULL DEFAULT '';
