-- GDPR user erasure leaves one content-free audit row per erasure in memory_control_events
-- (EP-8 S4, FU-03): event 'erased', with the subject_type, the number of profile rows and the number of
-- personal memories deleted in `detail`, never the erased text or an id list. The event CHECK is a closed
-- set, so it is replaced as 0100_user_profiles.sql replaced it; NOT VALID then VALIDATE, so the swap does
-- not hold a long exclusive lock. Every earlier event stays in the set.
ALTER TABLE memory_control_events DROP CONSTRAINT IF EXISTS memory_control_events_event_check;
ALTER TABLE memory_control_events
    ADD CONSTRAINT memory_control_events_event_check
    CHECK (event IN ('state_changed', 'reset', 'profile_edited', 'profile_rebuilt', 'erased')) NOT VALID;
ALTER TABLE memory_control_events VALIDATE CONSTRAINT memory_control_events_event_check;
