DELETE FROM resource_grants WHERE subject_type = 'space';
ALTER TABLE resource_grants DROP CONSTRAINT IF EXISTS resource_grants_space_role_chk;
ALTER TABLE resource_grants DROP CONSTRAINT IF EXISTS resource_grants_subject_type_chk;
ALTER TABLE resource_grants
    ADD CONSTRAINT resource_grants_subject_type_chk
    CHECK (subject_type IN ('user', 'team'));
