-- Reverses 0057_org_learning_policy_mirror.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- Dropping the table discards the mirrored policies and every watermark. A build
-- that reads the table must be stopped first, or its memory gate fails with
-- "relation org_learning_policy_mirror does not exist" and every memory read and
-- write is refused until it restarts. After a restart on the older build nothing
-- reads org policy, so an org that disabled memory in org-core is remembered
-- again: unset SESSION_ORG_CORE_SERVICE_TOKEN and ORG_POLICY_FAIL_CLOSED first,
-- which restores the same behaviour while keeping the rows.
--
-- ORDER, all three steps:
--   1. roll session-core back to a build that does not read the mirror;
--   2. run this file;
--   3. the older build never mentions the table, so no restart is needed for it.
--
-- Safe to run twice (IF EXISTS).

BEGIN;
DROP TABLE IF EXISTS org_learning_policy_mirror;
COMMIT;
