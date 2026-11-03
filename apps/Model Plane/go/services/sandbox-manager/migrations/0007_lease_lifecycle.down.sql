ALTER TABLE leases DROP COLUMN IF EXISTS heartbeat_at;
ALTER TABLE leases DROP COLUMN IF EXISTS last_activity_at;
ALTER TABLE leases DROP COLUMN IF EXISTS failure_cause;
