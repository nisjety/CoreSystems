-- Reverses 0117_org_routing_overlays.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction. Then delete the version row
-- so the next boot re-applies 0117 cleanly (docs/runbooks/migration-policy.md, "Rolling back past a contract
-- migration").
--
-- Dropping the table discards every organisation's overlay. A build that reads the table must be stopped first,
-- or the org overlay RPCs and the organisation purge fail with "relation org_routing_overlays does not exist".
-- Setting SESSION_ORG_ROUTING_OVERLAY off stops the RPCs reading it, but the purge deletes from it whatever the
-- switch says, so roll session-core back first.
--
-- ORDER, all three steps:
--   1. roll session-core back to a build that does not mention org_routing_overlays;
--   2. run this file;
--   3. DELETE FROM _sqlx_migrations WHERE version = 117.
--
-- Safe to run twice (IF EXISTS).

BEGIN;
DROP TABLE IF EXISTS org_routing_overlays;
COMMIT;
