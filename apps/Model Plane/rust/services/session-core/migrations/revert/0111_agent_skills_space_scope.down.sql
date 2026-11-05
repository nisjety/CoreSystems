-- Reverses 0111_agent_skills_space_scope.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- DESTROYS every Space skill (rows of scope 'space', with their versions through
-- 0108's delete path; their revision log keeps a `deleted` entry). Org and user
-- skills are untouched. Export the Space skills first if they matter:
--
--   SELECT * FROM agent_skills WHERE scope = 'space';
--
-- Run it before revert/0109_authored_behaviour_kind.down.sql, which drops the
-- scope_id column this file's CHECK reads.
--
-- ORDER, all three steps:
--   1. set SKILL_SPACE_SCOPE unset (or not `true`) for session-core, capability-core
--      and model-gateway, so nothing lists or writes a Space row;
--   2. run this file;
--   3. delete the 0111 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (IF EXISTS).

BEGIN;
SELECT set_config('mp.actor_kind', 'system', true), set_config('mp.actor_id', 'revert:0111', true);
DELETE FROM agent_skills WHERE scope = 'space';
DROP INDEX IF EXISTS agent_skills_space_idx;
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_space_owner_chk;
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_scope_id_chk;
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_scope_chk;
ALTER TABLE agent_skills
    ADD CONSTRAINT agent_skills_scope_chk
    CHECK (scope IN ('org', 'user'));
COMMIT;
