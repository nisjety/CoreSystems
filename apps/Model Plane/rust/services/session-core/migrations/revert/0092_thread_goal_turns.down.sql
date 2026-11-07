-- Reverses 0092_thread_goal_turns.sql: drops the turn ledger and the goal counters.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- DESTROYS every goal's turn history and its spent counters (the goals themselves stay, with no
-- record of what they spent). Export them first if they matter.
--
-- ORDER, all three steps:
--   1. set MODEL_GATEWAY_THREAD_GOALS_ENABLED and SESSION_CORE_THREAD_GOALS_ENABLED unset (or not
--      `true`), and recreate model-gateway and session-core;
--   2. roll session-core back to a build without the goal turn ledger (its goal reads name the
--      counter columns);
--   3. run this file.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

ALTER TABLE thread_goals DROP COLUMN IF EXISTS turns_used;
ALTER TABLE thread_goals DROP COLUMN IF EXISTS cost_used_usd_micros;
ALTER TABLE thread_goals DROP COLUMN IF EXISTS tokens_used;
DROP INDEX IF EXISTS thread_goal_turns_open_idx;
DROP INDEX IF EXISTS thread_goal_turns_run_idx;
DROP TABLE IF EXISTS thread_goal_turns;

COMMIT;
