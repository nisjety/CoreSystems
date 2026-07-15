ALTER TABLE auth_membership_projection_versions
  DROP CONSTRAINT IF EXISTS auth_membership_projection_role_state_check,
  DROP COLUMN IF EXISTS desired_role;

ALTER TABLE auth_organization_projection_versions
  DROP COLUMN IF EXISTS desired_metadata,
  DROP COLUMN IF EXISTS desired_owner_user_id,
  DROP COLUMN IF EXISTS desired_slug,
  DROP COLUMN IF EXISTS desired_name;
