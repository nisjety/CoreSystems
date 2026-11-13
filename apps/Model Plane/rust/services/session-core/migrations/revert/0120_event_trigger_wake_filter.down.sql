-- Reverses 0120_event_trigger_wake_filter.sql: drops the wake filter column.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set EVENT_TRIGGER_WAKE_GATE_ENABLED to anything but true on capability-core (or roll that build back to one
-- without the wake gate) first, and roll session-core back to a build that neither reads nor writes the column. With
-- the switch on, every sweep reads the column, so a sweep after the drop fails. The column holds only the members'
-- filters, so nothing is copied first, and dropping it leaves every trigger unfiltered.
--
-- Safe to run twice (the DROP is IF EXISTS).

BEGIN;

ALTER TABLE event_triggers DROP COLUMN IF EXISTS wake_filter;

COMMIT;
