-- Reverses 0137_space_run_approval_phase_events.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only, inside
-- one transaction. Safe to run twice. After it, the approval phase is derived on read only
-- (run_phase_sql) and no run.state_changed row is written when an approval opens or closes.

BEGIN;

DROP TRIGGER IF EXISTS space_event_approvals_phase_upd ON approvals;
DROP TRIGGER IF EXISTS space_event_approvals_phase_ins ON approvals;
DROP FUNCTION IF EXISTS space_event_from_approvals_phase();

COMMIT;
