DROP TABLE IF EXISTS organization_plan_change_outbox;
ALTER TABLE organizations
  DROP CONSTRAINT IF EXISTS organizations_plan_revision_nonnegative;
ALTER TABLE organizations
  DROP COLUMN IF EXISTS plan_revision;
