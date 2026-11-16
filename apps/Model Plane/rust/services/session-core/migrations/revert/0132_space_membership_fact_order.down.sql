-- Reverses 0132_space_membership_fact_order.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Drops the ordering table. Roll session-core back first: a build that applies membership facts reads and writes it,
-- and fails without it. The revocation projection (space_membership_revocations) is not touched.
--
-- Safe to run twice.

BEGIN;
DROP TABLE IF EXISTS space_membership_fact_order;
COMMIT;
