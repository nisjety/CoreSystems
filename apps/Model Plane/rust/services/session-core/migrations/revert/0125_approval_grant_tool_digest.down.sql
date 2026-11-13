-- Reverses 0125_approval_grant_tool_digest.sql: drops the grant's tool digest binding.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the
-- top level of migrations/. Run it by hand, against the database only.
--
-- ORDER, both steps:
--   1. turn APPROVAL_SCOPE_GRANTS_ENABLED off on session-core and execution-core, or roll
--      session-core back to a build that does not read or write tool_digest, so no grant is
--      created or matched while the column goes;
--   2. run this file.
--
-- The column holds a registry digest and no content, so nothing needs copying first. After the
-- drop a grant matches on owner, thread, tool, action key and class again, as before 0125.
--
-- Safe to run twice (every statement is IF EXISTS).

BEGIN;

ALTER TABLE approval_scope_grants DROP CONSTRAINT IF EXISTS approval_scope_grants_tool_digest_shape;
ALTER TABLE approval_scope_grants DROP COLUMN IF EXISTS tool_digest;

COMMIT;
