-- Reverses 0093_thread_goal_verdicts.sql: drops the verdict columns of the goal turn ledger.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- DESTROYS every judge verdict, its reasons, its evidence ids and the judge model names. The turns
-- and their counts stay (judge_tokens is 0092's column and stays too). Export them first if they
-- matter.
--
-- ORDER, all three steps:
--   1. set SESSION_CORE_THREAD_GOALS_ENABLED unset (or not `true`), stop every caller of
--      RecordGoalVerdict, and recreate session-core;
--   2. roll session-core back to a build without RecordGoalVerdict (its goal reads name the
--      verdict column, GOAL_COLUMNS in thread_goals.rs);
--   3. run this file.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

ALTER TABLE thread_goal_turns DROP COLUMN IF EXISTS judge_model;
ALTER TABLE thread_goal_turns DROP COLUMN IF EXISTS evidence_ids;
ALTER TABLE thread_goal_turns DROP COLUMN IF EXISTS verdict_reasons;
ALTER TABLE thread_goal_turns DROP COLUMN IF EXISTS verdict;

COMMIT;
