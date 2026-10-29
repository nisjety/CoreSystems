-- 0005_evidence_run_guard.sql
-- Evidence (search pages, notice bodies and download attempts) is written only under a
-- run that is running. A process whose run was recovered or has ended can no longer add
-- rows under that run's id, so a run's record closes its audit trail: no evidence row
-- carries a run id whose run had ended before the row was written.
--
-- The check takes a share lock on the run's row (FOR SHARE). A run cannot be ended by a
-- concurrent transaction between this check and the commit of the insert, and an insert
-- cannot start beside an UPDATE that is ending the run: whichever comes second waits,
-- and when the insert is the one that waits, it is refused once the run has ended.
--
-- The refusal is SQLSTATE 55000 (object_not_in_prerequisite_state). Its message names the
-- table and the run id and nothing from the row: a row can hold a notice's contact details.
-- INSERT ... ON CONFLICT DO NOTHING fires a BEFORE INSERT trigger before the conflict is
-- looked for, so a row that is already held is refused too. That is intended.
--
-- Forward-only. Runs inside the caller's transaction (migrate.go, applyOne), so no BEGIN,
-- COMMIT or ROLLBACK. migrate_concurrency_test.go drops this function before each round.

CREATE FUNCTION evidence_run_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM 1 FROM sync_runs WHERE id = NEW.run_id AND state = 'running' FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'INSERT into % refused: run % is not running', TG_TABLE_NAME, NEW.run_id
            USING ERRCODE = 'object_not_in_prerequisite_state';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER raw_search_pages_run_guard BEFORE INSERT ON raw_search_pages
    FOR EACH ROW EXECUTE FUNCTION evidence_run_guard();
CREATE TRIGGER raw_documents_run_guard BEFORE INSERT ON raw_documents
    FOR EACH ROW EXECUTE FUNCTION evidence_run_guard();
CREATE TRIGGER download_attempts_run_guard BEFORE INSERT ON download_attempts
    FOR EACH ROW EXECUTE FUNCTION evidence_run_guard();

COMMENT ON FUNCTION evidence_run_guard() IS 'Refuses an evidence insert whose run_id does not name a run in state running. SQLSTATE 55000; the message names the table and the run id, never row data.';
