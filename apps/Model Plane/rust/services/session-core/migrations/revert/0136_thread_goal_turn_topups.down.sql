-- Reverses 0136_thread_goal_turn_topups.sql: drops the goal turn top-up ledger and the run lookup index.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- DESTROYS the record of which continuation segments were debited to a goal. The goals' counters stay as they are
-- (the usage already added is not taken back); with the table gone a retried segment could be debited again, so turn
-- the caller off first.
--
-- ORDER:
--   1. set MODEL_GATEWAY_GOAL_SEGMENT_TOPUP unset (or not `true`), and recreate model-gateway;
--   2. roll session-core back to a build without TopUpGoalTurn;
--   3. run this file.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS thread_goal_turns_run_lookup_idx;
DROP TABLE IF EXISTS thread_goal_turn_topups;

COMMIT;
