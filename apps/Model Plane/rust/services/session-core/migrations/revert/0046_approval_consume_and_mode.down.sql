-- Reverses 0046_approval_consume_and_mode.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction. Safe only while no loop-mode approval is
-- pending: a loop-mode approval loses its mode and would fall to the cold worker.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;
ALTER TABLE approvals
    DROP COLUMN IF EXISTS consumed_claim_hash,
    DROP COLUMN IF EXISTS consumed_at,
    DROP COLUMN IF EXISTS continuation_mode;
COMMIT;
