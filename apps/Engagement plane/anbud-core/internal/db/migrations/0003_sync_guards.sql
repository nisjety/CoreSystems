-- 0003_sync_guards.sql
-- A run's provenance is fixed once it starts: its id, mode, window and start time
-- say what the run was asked to do, so the ledger must not rewrite them. State,
-- end time, retry instant, last error and counters stay mutable, because a run
-- has to finish and FinishRun writes them.
-- Runs inside the caller's transaction (migrate.go:167-175), so no BEGIN, COMMIT or ROLLBACK.
-- Migration 0002 is not edited. migrate_concurrency_test.go drops this function before each round.

CREATE FUNCTION sync_runs_provenance_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
        OR NEW.mode IS DISTINCT FROM OLD.mode
        OR NEW.range_from IS DISTINCT FROM OLD.range_from
        OR NEW.range_to IS DISTINCT FROM OLD.range_to
        OR NEW.started_at IS DISTINCT FROM OLD.started_at THEN
        RAISE EXCEPTION 'UPDATE of sync_runs provenance refused: id, mode, window and start are fixed'
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER sync_runs_provenance BEFORE UPDATE ON sync_runs
    FOR EACH ROW EXECUTE FUNCTION sync_runs_provenance_guard();

COMMENT ON FUNCTION sync_runs_provenance_guard() IS 'Refuses a change to the id, mode, window or start time of a sync run.';
