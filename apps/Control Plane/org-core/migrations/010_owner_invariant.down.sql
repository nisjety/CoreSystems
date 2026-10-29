DROP TRIGGER IF EXISTS organization_members_preserve_owner ON organization_members;
DROP TRIGGER IF EXISTS organizations_require_owner ON organizations;
DROP FUNCTION IF EXISTS enforce_live_organization_owner();
DROP FUNCTION IF EXISTS assert_live_organization_has_owner(TEXT);
