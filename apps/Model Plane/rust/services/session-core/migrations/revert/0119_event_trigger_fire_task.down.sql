-- Reverses 0119_event_trigger_fire_task.sql: drops the starter's claim index and the task columns.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set EVENT_TRIGGER_RUN_ENABLED to anything but true on capability-core (or roll it back to a build without the
-- trigger starter) first, or the starter fails on the missing columns. The columns hold only a task id and a time, and
-- the tasks themselves live in the tasks table, so nothing is copied first. Dropping them leaves every authorised fire
-- unstarted.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS event_trigger_fires_unstarted_idx;
ALTER TABLE event_trigger_fires
    DROP COLUMN IF EXISTS started_at,
    DROP COLUMN IF EXISTS task_id;

COMMIT;
