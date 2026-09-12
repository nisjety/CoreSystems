DROP TABLE IF EXISTS sandbox_process_output;
DROP INDEX IF EXISTS sandbox_processes_heartbeat_idx;
DROP INDEX IF EXISTS sandbox_processes_live_idx;
DROP INDEX IF EXISTS sandbox_processes_lease_idx;
DROP INDEX IF EXISTS sandbox_processes_space_idx;
DROP TABLE IF EXISTS sandbox_processes;
ALTER TABLE leases DROP COLUMN IF EXISTS processes_permitted;
