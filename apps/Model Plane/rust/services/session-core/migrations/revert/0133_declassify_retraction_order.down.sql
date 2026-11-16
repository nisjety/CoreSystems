-- Reverses 0133_declassify_retraction_order.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Roll session-core back first: a build that declassifies or retracts reads and writes both objects. Dropping the
-- column discards every recorded share revision, and the table discards the retraction marks. The memory ledger
-- (memory_declassifications) and the revocation projection (space_membership_revocations) are otherwise not touched.
--
-- Safe to run twice.

BEGIN;
DROP TABLE IF EXISTS space_membership_retract_marks;
ALTER TABLE memory_declassifications DROP COLUMN IF EXISTS authority_revision;
COMMIT;
