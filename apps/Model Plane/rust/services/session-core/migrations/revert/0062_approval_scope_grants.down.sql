-- Reverses 0062_approval_scope_grants.sql: drops the grant table and its indexes.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the
-- top level of migrations/. Run it by hand, against the database only.
--
-- A grant is authority: while APPROVAL_SCOPE_GRANTS_ENABLED is on, a live row releases a call
-- without a pause. Turn that switch off on every instance first, then look at what would go,
-- read-only:
--
--   SELECT scope, count(*) FROM approval_scope_grants WHERE revoked_at IS NULL GROUP BY scope;
--
-- ORDER, both steps:
--   1. turn APPROVAL_SCOPE_GRANTS_ENABLED off on session-core and execution-core, or roll them
--      back to a build without the grant RPCs (no caller then asks for a release);
--   2. run this file.

DROP INDEX IF EXISTS approval_scope_grants_thread_idx;
DROP INDEX IF EXISTS approval_scope_grants_live_idx;
DROP TABLE IF EXISTS approval_scope_grants;
