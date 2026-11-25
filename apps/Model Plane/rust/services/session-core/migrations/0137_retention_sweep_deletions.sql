-- 0137: retention sweep deletion ledger (F-f10-10).
--
-- The retention sweeper (src/retention_sweeper.rs) deletes each thread in its own transaction but
-- wrote its retention_sweep_runs row only once, at the end of the org's pass. A crash, a restart or a
-- failed record left committed deletes with no run row, and since the daily cap summed
-- retention_sweep_runs.deleted those deletes did not count against SESSION_RETENTION_SWEEP_DAILY_CAP
-- either. From this migration on every enforce delete writes one ledger row in the same transaction
-- as the delete:
--
--   * the daily cap counts the org's ledger rows of the last 24 hours, so it no longer depends on a
--     run row (retention_sweep_runs.deleted stays, for reporting);
--   * the next pass records every run id that has ledger rows but no run row, once its last delete is
--     older than 15 minutes, as an 'interrupted' run with the ledger count as `deleted`, so an audit
--     event exists for every delete;
--   * ledger rows older than 48 hours whose run row exists are pruned. A row whose run is not recorded
--     yet is never pruned.
--
-- Ids and a timestamp only: no thread id, title or content, so Zero Data Retention has nothing to
-- refuse here and a waived run is no different.
--
-- retention_sweep_runs gains the error code 'interrupted'. It is the one error that carries deletes:
-- an enforce row with zero candidates (so zero held, skipped and would-delete counts) and deleted > 0.
-- Every other error still reports zero counts. The two CHECKs that name error_code were unnamed in
-- 0107, so they are found by the column they read and replaced by named ones; added NOT VALID and then
-- validated, as 0100 does, so the swap does not hold a long exclusive lock.
--
-- Additive apart from the widened CHECKs. The reversal is in
-- revert/0137_retention_sweep_deletions.down.sql.

CREATE TABLE IF NOT EXISTS retention_sweep_deletions (
    run_id TEXT NOT NULL CHECK (run_id LIKE 'rsw\_%' AND length(run_id) <= 64),
    org_id TEXT NOT NULL CHECK (length(org_id) BETWEEN 1 AND 128),
    deleted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The daily cap: one org's rows of the last 24 hours.
CREATE INDEX IF NOT EXISTS idx_retention_sweep_deletions_org
    ON retention_sweep_deletions (org_id, deleted_at);

-- The reconcile and the prune look rows up by run.
CREATE INDEX IF NOT EXISTS idx_retention_sweep_deletions_run
    ON retention_sweep_deletions (run_id);

DO $$
DECLARE
    con record;
BEGIN
    FOR con IN
        SELECT c.conname
          FROM pg_constraint c
         WHERE c.conrelid = 'retention_sweep_runs'::regclass
           AND c.contype = 'c'
           AND c.conkey @> ARRAY[(SELECT a.attnum
                                    FROM pg_attribute a
                                   WHERE a.attrelid = 'retention_sweep_runs'::regclass
                                     AND a.attname = 'error_code')]
           AND c.conname NOT IN ('retention_sweep_runs_error_code_chk',
                                 'retention_sweep_runs_error_counts_chk')
    LOOP
        EXECUTE format('ALTER TABLE retention_sweep_runs DROP CONSTRAINT %I', con.conname);
    END LOOP;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'retention_sweep_runs_error_code_chk'
           AND conrelid = 'retention_sweep_runs'::regclass
    ) THEN
        ALTER TABLE retention_sweep_runs
            ADD CONSTRAINT retention_sweep_runs_error_code_chk
            CHECK (error_code IS NULL
                   OR error_code IN ('policy_unavailable', 'policy_window_invalid', 'policy_changed',
                                     'interrupted')) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'retention_sweep_runs_error_counts_chk'
           AND conrelid = 'retention_sweep_runs'::regclass
    ) THEN
        -- A failed pass reports nothing it could not stand behind; an interrupted one reports only
        -- the deletes its ledger proves.
        ALTER TABLE retention_sweep_runs
            ADD CONSTRAINT retention_sweep_runs_error_counts_chk
            CHECK (error_code IS NULL
                   OR (error_code = 'interrupted' AND mode = 'enforce' AND candidates = 0 AND deleted > 0)
                   OR (error_code <> 'interrupted' AND candidates = 0 AND deleted = 0)) NOT VALID;
    END IF;
END
$$;

ALTER TABLE retention_sweep_runs VALIDATE CONSTRAINT retention_sweep_runs_error_code_chk;
ALTER TABLE retention_sweep_runs VALIDATE CONSTRAINT retention_sweep_runs_error_counts_chk;
