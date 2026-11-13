-- Reverses 0121_memory_control_events_erased.sql: puts the event CHECK back to its 0100 set.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build that does not write the 'erased' audit event first, or its
-- user-erasure consumer fails against the narrower CHECK. The 'erased' rows hold only counts, so they are
-- deleted before the CHECK is put back (a validated CHECK would refuse them).
--
-- Safe to run twice (the DELETE matches nothing the second time, and the DROP is IF EXISTS).

BEGIN;

DELETE FROM memory_control_events WHERE event = 'erased';

ALTER TABLE memory_control_events DROP CONSTRAINT IF EXISTS memory_control_events_event_check;
ALTER TABLE memory_control_events
    ADD CONSTRAINT memory_control_events_event_check
    CHECK (event IN ('state_changed', 'reset', 'profile_edited', 'profile_rebuilt'));

COMMIT;
