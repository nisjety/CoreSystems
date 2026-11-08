-- Reverses 0113_event_triggers.sql: drops the event trigger table and its indexes.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set EVENT_TRIGGERS_ENABLED to anything but true (or roll session-core back to a build without
-- src/event_triggers.rs) first, or the trigger RPCs fail on the missing table. The rows are the member's own
-- trigger records and name nothing else, so nothing is copied first.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS event_triggers_connection_idx;
DROP INDEX IF EXISTS event_triggers_member_idx;
DROP TABLE IF EXISTS event_triggers;

COMMIT;
