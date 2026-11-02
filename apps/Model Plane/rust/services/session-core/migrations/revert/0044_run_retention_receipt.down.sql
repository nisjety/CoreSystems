-- Reverses 0044_run_retention_receipt.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction. It removes the receipts: a run that was waived
-- is then indistinguishable from an ordinary one, so run it only when no
-- waived run is still under review.
--
-- Safe to run twice (every statement is IF EXISTS).
--
-- Ordering with 0072_space_event_log: once 0072 is installed, its two runs triggers
-- (space_event_runs_ins / space_event_runs_upd) carry a WHEN clause on
-- runs.retention_class, so Postgres refuses to drop that column while they exist. This
-- revert therefore drops those two triggers first. That is the whole of the 0072
-- dependency: its functions, log tables and the messages/threads triggers do not read
-- the column and are left alone. After this revert the Space event log no longer records
-- run lifecycle events; re-apply 0072_space_event_log.sql only together with 0044, since
-- 0072's runs triggers cannot be created without the column. To remove the whole Space
-- event log, run revert/0072_space_event_log.down.sql (also safe in either order).

BEGIN;

DROP TRIGGER IF EXISTS space_event_runs_ins ON runs;
DROP TRIGGER IF EXISTS space_event_runs_upd ON runs;

DROP INDEX IF EXISTS runs_zdr_waived_thread_idx;
DROP TRIGGER IF EXISTS runs_retention_receipt_immutable_trg ON runs;
DROP FUNCTION IF EXISTS runs_retention_receipt_immutable();
ALTER TABLE runs DROP CONSTRAINT IF EXISTS runs_retention_waiver_consistent;
ALTER TABLE runs
    DROP COLUMN IF EXISTS waiver_connection_id,
    DROP COLUMN IF EXISTS waiver_ack_id,
    DROP COLUMN IF EXISTS retention_class;

COMMIT;
