-- Rollback organization membership tracking

-- Drop functions first (they depend on the table)
DROP FUNCTION IF EXISTS get_user_organizations(TEXT);
DROP FUNCTION IF EXISTS remove_organization_member(TEXT, TEXT);
DROP FUNCTION IF EXISTS add_organization_member(TEXT, TEXT, TEXT, TEXT);

-- Drop indexes
DROP INDEX IF EXISTS idx_org_members_user_status;
DROP INDEX IF EXISTS idx_org_members_status;
DROP INDEX IF EXISTS idx_org_members_role;
DROP INDEX IF EXISTS idx_org_members_user_id;
DROP INDEX IF EXISTS idx_org_members_org_id;

-- Drop the table
DROP TABLE IF EXISTS organization_members;
