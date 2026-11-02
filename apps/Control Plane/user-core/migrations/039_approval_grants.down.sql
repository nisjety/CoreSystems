DROP TABLE IF EXISTS space_approval_grant_events;
DROP TABLE IF EXISTS space_approval_grants;
DROP TABLE IF EXISTS space_approval_policies;
ALTER TABLE space_effect_policies DROP COLUMN IF EXISTS approval_grant_entitled;
