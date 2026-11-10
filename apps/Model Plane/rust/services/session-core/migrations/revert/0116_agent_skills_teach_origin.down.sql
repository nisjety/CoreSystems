-- Reverses 0116_agent_skills_teach_origin.sql: restores the origin CHECK that 0006 left, which accepts 'user'
-- and 'background_review' only.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction. Then delete the version row
-- so the next boot re-applies 0116 cleanly (docs/runbooks/migration-policy.md, "Rolling back past a contract
-- migration").
--
-- Valid ONLY while no agent_skills row holds origin 'teach': re-adding the narrower CHECK validates every
-- existing row. The DO block refuses rather than deleting anything, because a taught skill is a person's
-- proposal. Check first, read-only:
--
--   SELECT count(*) FROM agent_skills WHERE origin = 'teach';
--
-- ORDER, all three steps:
--   1. roll session-core back to a build that no longer accepts origin 'teach', or make sure no teach write was
--      accepted (no teach caller may be switched on while this runs);
--   2. confirm the query above returns 0;
--   3. run this file, then DELETE FROM _sqlx_migrations WHERE version = 116.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM agent_skills WHERE origin = 'teach') THEN
        RAISE EXCEPTION 'agent_skills still holds origin teach rows: reject or export them before reverting 0116';
    END IF;
END
$$;

ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_origin_chk;

ALTER TABLE agent_skills
    ADD CONSTRAINT agent_skills_origin_chk
    CHECK (origin IN ('user', 'background_review')) NOT VALID;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_origin_chk;

COMMIT;
