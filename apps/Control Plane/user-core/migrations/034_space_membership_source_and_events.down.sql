DROP TABLE IF EXISTS space_membership_events;
ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_roster_mode_chk;
ALTER TABLE registered_spaces DROP COLUMN IF EXISTS roster_mode;
ALTER TABLE space_memberships DROP CONSTRAINT IF EXISTS space_memberships_source_chk;
ALTER TABLE space_memberships DROP COLUMN IF EXISTS source;
