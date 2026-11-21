-- O-21: the compliance organisation role. Widens the Auth Core membership
-- projection's role CHECK (015) to list 'compliance'. Whether org-core accepts the
-- role is decided by ORG_COMPLIANCE_ROLE_ENABLED (default off); this only lets a
-- projected row carry it. Idempotent: the constraint is dropped if present and
-- re-created. NOT VALID, as 015 made it, so the existing rows are not rescanned.

ALTER TABLE auth_membership_projection_versions
  DROP CONSTRAINT IF EXISTS auth_membership_projection_role_state_check;
ALTER TABLE auth_membership_projection_versions
  ADD CONSTRAINT auth_membership_projection_role_state_check CHECK (
    (desired_action = 'remove' AND desired_role IS NULL) OR
    (desired_action = 'upsert' AND desired_role IN (
      'owner', 'admin', 'member', 'viewer', 'compliance'
    ))
  ) NOT VALID;
