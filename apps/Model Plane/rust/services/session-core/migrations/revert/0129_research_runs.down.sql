-- Reverses 0115_research_runs.sql: drops the research run table and its indexes.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build without src/research_runs.rs first, or the research run callers fail on
-- the missing table. The table holds the partial results of non-ZDR research runs and nothing copies it first:
-- dropping it discards those results and the run records.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS research_runs_deadline_idx;
DROP INDEX IF EXISTS research_runs_org_idx;
DROP INDEX IF EXISTS research_runs_thread_idx;
DROP INDEX IF EXISTS research_runs_one_running_idx;
DROP TABLE IF EXISTS research_runs;

COMMIT;
