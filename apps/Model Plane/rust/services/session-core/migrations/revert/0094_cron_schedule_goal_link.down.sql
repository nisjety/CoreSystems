-- Reverses 0094_cron_schedule_goal_link.sql: drops the goal link of schedules, its triggers and
-- the unattended marks of goal turns.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- A goal-bound schedule keeps its template (which still names the goal) but loses the link, so
-- its fires stop being ledgered on the goal and a goal close no longer pauses it. Step 2 below
-- disables every such schedule first, so none runs on as an ordinary schedule. The unattended
-- turns stay in the ledger as ordinary settled turns (their counts are kept).
--
-- ORDER, all four steps:
--   1. set MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED and ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED unset
--      (or not `true`), and recreate model-gateway and orchestrator-core;
--   2. roll session-core back to a build without the unattended goal turn (its scheduled result
--      write names the new columns);
--   3. run this file;
--   4. check that no schedule whose template names a `goal_id` is enabled.
--
-- Safe to run twice (every DROP is IF EXISTS; the UPDATE runs only while the column exists).

BEGIN;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'cron_schedules' AND column_name = 'goal_id'
    ) THEN
        UPDATE cron_schedules SET enabled = false, updated_at = now()
        WHERE goal_id IS NOT NULL AND enabled;
    END IF;
END;
$$;

DROP TRIGGER IF EXISTS thread_goals_pause_schedules ON thread_goals;
DROP TRIGGER IF EXISTS cron_schedules_guard_goal ON cron_schedules;
DROP TRIGGER IF EXISTS cron_schedules_bind_goal ON cron_schedules;
DROP FUNCTION IF EXISTS thread_goals_pause_schedules();
DROP FUNCTION IF EXISTS cron_schedules_guard_goal();
DROP FUNCTION IF EXISTS cron_schedules_bind_goal();
DROP INDEX IF EXISTS cron_schedules_goal_idx;
ALTER TABLE thread_goal_turns DROP COLUMN IF EXISTS scheduled_thread_id;
ALTER TABLE thread_goal_turns DROP COLUMN IF EXISTS unattended;
ALTER TABLE cron_schedules DROP COLUMN IF EXISTS goal_paused_reason;
ALTER TABLE cron_schedules DROP COLUMN IF EXISTS goal_id;

COMMIT;
