-- Reverses 0064_skill_revisions.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- Dropping the table discards every skill revision: the ledger of what Verevon
-- learned and the before-snapshots an undo restores from. The skills themselves
-- are untouched. Dropping the trigger first means no skill write can fail on a
-- missing table while the file runs.
--
-- ORDER, all three steps:
--   1. roll session-core and capability-core back to builds that do not read
--      skill_revisions (the learning ledger RPCs name the table);
--   2. run this file;
--   3. delete the 0064 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (IF EXISTS).

BEGIN;
DROP TRIGGER IF EXISTS agent_skills_revision_trg ON agent_skills;
DROP FUNCTION IF EXISTS agent_skills_revision_fn();
DROP FUNCTION IF EXISTS agent_skill_revision_snapshot(agent_skills);
DROP TABLE IF EXISTS skill_revisions;
COMMIT;
