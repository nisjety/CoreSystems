-- Reverses 0108_agent_skill_versions.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- DESTROYS every skill version (the numbered, immutable bodies and their digests)
-- and the version, digest and bundle_sha256 columns of agent_skills. The skills
-- themselves and their revision log are untouched. Runs that recorded a pin
-- (skill_id, version, digest) keep it, and it then reads as gone. Export the
-- versions first if they matter. Dropping the triggers first means no skill write
-- can fail on a missing table while the file runs.
--
-- ORDER, all three steps:
--   1. set SKILL_VERSIONS_API unset (or not `true`) for capability-core and
--      session-core, and roll both back to builds that do not read the columns
--      (session-core's ListAgentSkills selects version and digest);
--   2. run this file;
--   3. delete the 0108 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (IF EXISTS).

BEGIN;
DROP TRIGGER IF EXISTS agent_skills_version_trg ON agent_skills;
DROP TRIGGER IF EXISTS agent_skills_version_bump_trg ON agent_skills;
DROP TRIGGER IF EXISTS agent_skill_versions_immutable_trg ON agent_skill_versions;
DROP FUNCTION IF EXISTS agent_skills_version_fn();
DROP FUNCTION IF EXISTS agent_skills_version_bump_fn();
DROP FUNCTION IF EXISTS agent_skill_versions_immutable_fn();
DROP FUNCTION IF EXISTS agent_skill_digest(agent_skills);
DROP FUNCTION IF EXISTS agent_skill_version_body(agent_skills);
DROP INDEX IF EXISTS agent_skill_versions_org_owner_idx;
DROP TABLE IF EXISTS agent_skill_versions;
ALTER TABLE agent_skills
    DROP COLUMN IF EXISTS bundle_sha256,
    DROP COLUMN IF EXISTS digest,
    DROP COLUMN IF EXISTS version;
COMMIT;
