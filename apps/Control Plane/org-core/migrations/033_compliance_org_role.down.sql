-- Reverses 033. Narrowing the CHECK back to the four roles is NOT VALID, so a
-- stored compliance projection row is kept, but a new compliance upsert is
-- refused again. Move compliance members to another role (in Auth Core) before
-- rolling back.
ALTER TABLE auth_membership_projection_versions
  DROP CONSTRAINT IF EXISTS auth_membership_projection_role_state_check;

ALTER TABLE auth_membership_projection_versions
  ADD CONSTRAINT auth_membership_projection_role_state_check CHECK (
    (desired_action = 'remove' AND desired_role IS NULL) OR
    (desired_action = 'upsert' AND desired_role IN (
      'owner', 'admin', 'member', 'viewer'
    ))
  ) NOT VALID;
