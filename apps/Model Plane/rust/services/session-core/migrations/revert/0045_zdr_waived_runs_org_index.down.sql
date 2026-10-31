-- Reverses 0045_zdr_waived_runs_org_index.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction. It removes only the index: the receipts are
-- unchanged, and the counts they feed are only slower to read.
--
-- Safe to run twice (IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS runs_zdr_waived_org_connection_idx;

COMMIT;
