-- D21i: 'compliance' becomes a fifth canonical organization role.
--
-- Auth Core is the membership authority (auth-core migration 031 lets
-- apply_membership_mutation grant, change and remove the role); org-core keeps
-- a projection of it. Widen the projection's role CHECK so a reconciled
-- compliance membership is not refused. Like 015 the constraint is added
-- NOT VALID: existing rows were already checked against the narrower set.
ALTER TABLE auth_membership_projection_versions
  DROP CONSTRAINT IF EXISTS auth_membership_projection_role_state_check;

ALTER TABLE auth_membership_projection_versions
  ADD CONSTRAINT auth_membership_projection_role_state_check CHECK (
    (desired_action = 'remove' AND desired_role IS NULL) OR
    (desired_action = 'upsert' AND desired_role IN (
      'owner', 'admin', 'member', 'viewer', 'compliance'
    ))
  ) NOT VALID;
