-- Reverses 033: restores the four-role CHECK from 015. NOT VALID, so a projection
-- row that already carries compliance stays readable, but it fails the CHECK on
-- its next write; move those members to another role (or turn
-- ORG_COMPLIANCE_ROLE_ENABLED off and let Auth Core re-project) before rolling back.
ALTER TABLE auth_membership_projection_versions
  DROP CONSTRAINT IF EXISTS auth_membership_projection_role_state_check;
ALTER TABLE auth_membership_projection_versions
  ADD CONSTRAINT auth_membership_projection_role_state_check CHECK (
    (desired_action = 'remove' AND desired_role IS NULL) OR
    (desired_action = 'upsert' AND desired_role IN (
      'owner', 'admin', 'member', 'viewer'
    ))
  ) NOT VALID;
