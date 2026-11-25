-- Reverses 0137_retention_sweep_deletions.sql: drops the retention sweep deletion ledger and restores
-- the two error_code CHECKs of 0107 (the three policy codes only, and an error row carries no counts).
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set SESSION_RETENTION_SWEEP=off, or roll session-core back to a build that neither writes the
-- ledger nor records 'interrupted' runs, first; otherwise the sweeper fails on the missing table.
--
-- An 'interrupted' row cannot satisfy the restored CHECKs, so it is kept as a plain enforce row instead
-- (error_code NULL, its deletes still in `deleted`): the older build's daily cap sums
-- retention_sweep_runs.deleted, so the deletes keep counting against the cap, and the row's audit event
-- already left through session_audit_outbox. Ledger rows whose run was never recorded are lost with the
-- table; check first, read-only, and let a pass record them (or wait until it has) before reverting:
--
--   SELECT count(DISTINCT d.run_id) FROM retention_sweep_deletions d
--    WHERE NOT EXISTS (SELECT 1 FROM retention_sweep_runs r WHERE r.id = d.run_id);
--
-- Safe to run twice (every DROP is IF EXISTS, and the UPDATE matches nothing the second time).

BEGIN;

UPDATE retention_sweep_runs SET error_code = NULL WHERE error_code = 'interrupted';

ALTER TABLE retention_sweep_runs DROP CONSTRAINT IF EXISTS retention_sweep_runs_error_code_chk;
ALTER TABLE retention_sweep_runs DROP CONSTRAINT IF EXISTS retention_sweep_runs_error_counts_chk;
ALTER TABLE retention_sweep_runs DROP CONSTRAINT IF EXISTS retention_sweep_runs_error_code_check;
ALTER TABLE retention_sweep_runs DROP CONSTRAINT IF EXISTS retention_sweep_runs_error_counts_check;

ALTER TABLE retention_sweep_runs
    ADD CONSTRAINT retention_sweep_runs_error_code_check
    CHECK (error_code IS NULL
           OR error_code IN ('policy_unavailable', 'policy_window_invalid', 'policy_changed'));

ALTER TABLE retention_sweep_runs
    ADD CONSTRAINT retention_sweep_runs_error_counts_check
    CHECK (error_code IS NULL OR (candidates = 0 AND deleted = 0));

DROP TABLE IF EXISTS retention_sweep_deletions;

COMMIT;
