BEGIN;

ALTER TABLE space_effect_policies
    DROP COLUMN IF EXISTS memory_declassify_entitled,
    DROP COLUMN IF EXISTS personal_agent_admit_entitled;

DROP TRIGGER IF EXISTS space_agent_admission_events_immutable_trg ON space_agent_admission_events;
DROP FUNCTION IF EXISTS space_agent_admission_events_immutable();
DROP TABLE IF EXISTS space_agent_admission_events;
DROP TABLE IF EXISTS space_agent_admissions;
DROP TABLE IF EXISTS agent_subject_access;
DROP TABLE IF EXISTS agent_subjects;

COMMIT;
