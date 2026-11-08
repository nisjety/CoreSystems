-- Reverses 0114_event_trigger_fires.sql: drops the event trigger fire table and its indexes.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set EVENT_TRIGGER_FIRE_ENABLED to anything but true (or roll capability-core back to a build without
-- internal/eventtrigger) first, or the sweep fails on the missing table every tick. The rows are fire records that
-- name only ids, a closed status and outcome, so nothing is copied first.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS event_trigger_fires_trigger_idx;
DROP INDEX IF EXISTS event_trigger_fires_org_idx;
DROP INDEX IF EXISTS event_trigger_fires_pending_idx;
DROP TABLE IF EXISTS event_trigger_fires;

COMMIT;
