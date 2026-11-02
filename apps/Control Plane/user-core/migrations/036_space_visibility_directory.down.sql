DROP INDEX IF EXISTS idx_registered_spaces_directory;
DROP TABLE IF EXISTS space_directory_policy_events;
DROP TABLE IF EXISTS space_directory_policies;
ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_join_role_chk;
ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_visibility_chk;
ALTER TABLE registered_spaces DROP COLUMN IF EXISTS join_role;
ALTER TABLE registered_spaces DROP COLUMN IF EXISTS visibility;
