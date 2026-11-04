-- Reverses 0091_thread_goals.sql: drops the thread_goals table and its indexes.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- DESTROYS every thread goal (objectives, criteria and states). Export them first if they matter.
--
-- ORDER, all three steps:
--   1. set MODEL_GATEWAY_THREAD_GOALS_ENABLED and SESSION_CORE_THREAD_GOALS_ENABLED unset (or not
--      `true`), and recreate model-gateway and session-core;
--   2. roll session-core back to a build without the thread_goals module (attention.rs reads the
--      table too while SESSION_CORE_THREAD_GOALS_ENABLED is on);
--   3. run this file.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS thread_goals_owner_idx;
DROP INDEX IF EXISTS thread_goals_thread_idx;
DROP INDEX IF EXISTS thread_goals_one_open_idx;
DROP TABLE IF EXISTS thread_goals;

COMMIT;
