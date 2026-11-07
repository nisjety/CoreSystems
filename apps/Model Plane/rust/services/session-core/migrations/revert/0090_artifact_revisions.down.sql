-- Reverses 0090_artifact_revisions.sql: drops the artifact_revisions table and its indexes.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- DESTROYS every stored artifact revision. While MODEL_GATEWAY_ARTIFACT_REVISIONS is `off` or
-- `shadow` nothing reads the table (message metadata still holds the latest version of each
-- artifact per turn), so the loss is only the extra history the table kept. Once it is `on`, user
-- edits and restores live only here: export them first.
--
-- ORDER, all three steps:
--   1. set MODEL_GATEWAY_ARTIFACT_REVISIONS=off and SESSION_CORE_ARTIFACT_REVISIONS_ENABLED unset,
--      and recreate model-gateway and session-core;
--   2. roll session-core back to a build without the artifact_revisions module;
--   3. run this file.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS artifact_revisions_org_idx;
DROP INDEX IF EXISTS artifact_revisions_idempotency_idx;
DROP INDEX IF EXISTS artifact_revisions_head_idx;
DROP TABLE IF EXISTS artifact_revisions;

COMMIT;
