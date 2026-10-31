-- Reverses 0058_agent_skills_proposals.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- Recreating the old unique key (org_id, name) fails when two rows now share a
-- name inside one org (a user-scoped skill beside an org skill of the same name,
-- or two people's skills of one name). The statements below do not delete data to
-- make it succeed: the index creation aborts the transaction and nothing changes.
-- Rename or delete the duplicate rows by hand first. This query lists them:
--
--   SELECT org_id, name, count(*) FROM agent_skills
--   GROUP BY org_id, name HAVING count(*) > 1;
--
-- Dropping review_state forgets which rows were proposals: every learned skill
-- still waiting for its owner reads as an ordinary row afterwards, and a proposal
-- is disabled and user-scoped, so it stays inert and private. Delete the
-- unwanted ones before the revert if that matters.
--
-- ORDER, all three steps:
--   1. roll session-core, model-gateway and capability-core back to builds that do
--      not read review_state (capability-core's skills SELECT names the column);
--   2. run this file;
--   3. delete the 0058 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (IF EXISTS / IF NOT EXISTS).

BEGIN;
CREATE UNIQUE INDEX IF NOT EXISTS agent_skills_org_name_uq
    ON agent_skills (org_id, name);
DROP INDEX IF EXISTS agent_skills_scope_owner_name_uq;
DROP INDEX IF EXISTS agent_skills_proposed_idx;
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_review_state_check;
ALTER TABLE agent_skills
    DROP COLUMN IF EXISTS review_state,
    DROP COLUMN IF EXISTS source_run_id,
    DROP COLUMN IF EXISTS source_thread_id,
    DROP COLUMN IF EXISTS proposal_reason;
COMMIT;
