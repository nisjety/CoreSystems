-- WS22-S6 lease lifecycle: the failure cause the latest thread-workspace apply
-- recorded, when the lease was last used, and the backend heartbeat that WS22-S8
-- will write. Idempotent: the entrypoint re-runs every up.sql at each boot, with
-- no ledger.
--
-- failure_cause stores model_plane.v1.SandboxFailureCause's numeric values, where
-- 0 records that no failure is recorded. An existing row reads as no failure, and
-- last_activity_at reads the migration time for it: the earliest moment this
-- build can vouch for.
ALTER TABLE leases ADD COLUMN IF NOT EXISTS failure_cause SMALLINT NOT NULL DEFAULT 0;
ALTER TABLE leases ADD COLUMN IF NOT EXISTS last_activity_at TIMESTAMPTZ NOT NULL DEFAULT now();
ALTER TABLE leases ADD COLUMN IF NOT EXISTS heartbeat_at TIMESTAMPTZ;
