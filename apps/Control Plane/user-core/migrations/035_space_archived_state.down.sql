UPDATE registered_spaces SET registration_state = 'active' WHERE registration_state = 'archived';
ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_state_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_state_chk
    CHECK (registration_state IN ('active', 'suspended', 'deleting', 'deleted'));
ALTER TABLE registered_spaces DROP COLUMN IF EXISTS archived_by;
ALTER TABLE registered_spaces DROP COLUMN IF EXISTS archived_at;
