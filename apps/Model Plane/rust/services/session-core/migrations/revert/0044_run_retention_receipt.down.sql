-- Reverses 0044_run_retention_receipt.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction. It removes the receipts: a run that was waived
-- is then indistinguishable from an ordinary one, so run it only when no
-- waived run is still under review.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS runs_zdr_waived_thread_idx;
DROP TRIGGER IF EXISTS runs_retention_receipt_immutable_trg ON runs;
DROP FUNCTION IF EXISTS runs_retention_receipt_immutable();
ALTER TABLE runs DROP CONSTRAINT IF EXISTS runs_retention_waiver_consistent;
ALTER TABLE runs
    DROP COLUMN IF EXISTS waiver_connection_id,
    DROP COLUMN IF EXISTS waiver_ack_id,
    DROP COLUMN IF EXISTS retention_class;

COMMIT;
