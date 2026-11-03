-- Reverses 0100_user_profiles.sql: drops the standing user profile store.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: switch SESSION_USER_PROFILE_ENABLED off and roll session-core back to a build without
-- MemoryService.GetUserProfile/SetUserProfile/RebuildUserProfile first: this build's ResetMemory, memory
-- delete and organization erasure also touch the table and fail on a missing one. The profile is derived
-- from agent_memory (an edited profile is the person's own text, which they can retype), so nothing is
-- copied first.
--
-- The two profile audit events are deleted before the event CHECK is put back to its 0051 set (a
-- validated CHECK would refuse them); they hold a revision and a line count, never content.
--
-- Safe to run twice (every DROP is IF EXISTS, and the DELETE matches nothing the second time).

BEGIN;

DROP TABLE IF EXISTS user_profiles;

DELETE FROM memory_control_events WHERE event IN ('profile_edited', 'profile_rebuilt');

ALTER TABLE memory_control_events DROP CONSTRAINT IF EXISTS memory_control_events_event_check;
ALTER TABLE memory_control_events
    ADD CONSTRAINT memory_control_events_event_check
    CHECK (event IN ('state_changed', 'reset'));

COMMIT;
