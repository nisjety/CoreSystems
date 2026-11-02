DROP INDEX IF EXISTS uq_registered_spaces_dm_participants;
ALTER TABLE space_effect_policies DROP COLUMN IF EXISTS dm_create_entitled;
ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_dm_hash_chk;
ALTER TABLE registered_spaces DROP COLUMN IF EXISTS participant_hash;
ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_kind_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_kind_chk
    CHECK (space_kind IN ('personal', 'room', 'project', 'case'));
