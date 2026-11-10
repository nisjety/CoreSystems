-- Reverses 0118_event_trigger_schedule.sql: drops the schedule binding index and column.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build that neither reads nor writes event_triggers.schedule_id (or set
-- EVENT_TRIGGERS_ENABLED to anything but true) first, or the trigger RPCs fail on the missing column. Any starter
-- that reads the binding must be off as well. The column holds only schedule ids, so nothing is copied first, and
-- dropping it leaves every trigger record-only.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS event_triggers_schedule_idx;
ALTER TABLE event_triggers DROP COLUMN IF EXISTS schedule_id;

COMMIT;
