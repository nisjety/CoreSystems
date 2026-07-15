-- Persist the canonical state attached to each Auth-owned revision so Org Core
-- can distinguish an exact retry from a conflicting same-revision payload.
-- Older rows remain nullable and therefore fail closed on an equal-revision
-- retry until Auth advances the aggregate revision during reconciliation.

ALTER TABLE auth_organization_projection_versions
  ADD COLUMN IF NOT EXISTS desired_name TEXT,
  ADD COLUMN IF NOT EXISTS desired_slug TEXT,
  ADD COLUMN IF NOT EXISTS desired_owner_user_id TEXT,
  ADD COLUMN IF NOT EXISTS desired_metadata JSONB;

UPDATE auth_organization_projection_versions version
SET desired_name = organization.name,
    desired_slug = COALESCE(organization.slug, ''),
    desired_owner_user_id = owner.user_id,
    desired_metadata = COALESCE(organization.metadata, '{}'::JSONB)
FROM organizations organization
LEFT JOIN LATERAL (
  SELECT member.user_id
  FROM organization_members member
  WHERE member.org_id = organization.id
    AND member.status = 'active'
    AND 'owner' = ANY(string_to_array(member.role, ','))
  ORDER BY member.created_at, member.id
  LIMIT 1
) owner ON TRUE
WHERE version.org_id = organization.id
  AND version.desired_name IS NULL;

ALTER TABLE auth_membership_projection_versions
  ADD COLUMN IF NOT EXISTS desired_role TEXT;

UPDATE auth_membership_projection_versions version
SET desired_role = CASE
      WHEN version.desired_action = 'upsert' THEN member.role
      ELSE NULL
    END
FROM organization_members member
WHERE version.org_id = member.org_id
  AND version.user_id = member.user_id
  AND version.desired_role IS NULL;

ALTER TABLE auth_membership_projection_versions
  ADD CONSTRAINT auth_membership_projection_role_state_check CHECK (
    (desired_action = 'remove' AND desired_role IS NULL) OR
    (desired_action = 'upsert' AND desired_role IN (
      'owner', 'admin', 'member', 'viewer'
    ))
  ) NOT VALID;
