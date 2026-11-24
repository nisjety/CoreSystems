-- Reverses 0139_space_membership_fact_role.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Roll session-core back first: a build that records the fact state writes these columns. The ordering itself
-- (space_seq) is kept.
--
-- Safe to run twice.

BEGIN;
DROP INDEX IF EXISTS space_membership_fact_order_below_editor_idx;
ALTER TABLE space_membership_fact_order DROP COLUMN IF EXISTS role, DROP COLUMN IF EXISTS active;
COMMIT;
