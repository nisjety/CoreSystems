-- ============================================================================
-- 008 DOWN — fully reverse the RLS tenant-isolation backstop.
-- Idempotent and safe to run whether or not 008 ever enabled RLS: every DROP
-- uses IF EXISTS and DISABLE on a table without RLS is a no-op. This restores
-- the pre-008 behavior (no row-level filtering) on every listed table.
-- ============================================================================

DO $rls_down$
DECLARE
    t TEXT;
    p TEXT;
BEGIN
    FOR t, p IN
        SELECT * FROM (VALUES
            ('organizations',        'org_isolation'),
            ('org_entitlements',     'org_ent_isolation'),
            ('organization_members', 'org_members_isolation'),
            ('org_role_mappings',    'org_roles_isolation'),
            ('org_tenant_links',     'org_tenant_links_isolation'),
            ('org_onboarding_states','org_onboarding_isolation')
        ) AS x(tbl, pol)
    LOOP
        IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = t) THEN
            EXECUTE format('DROP POLICY IF EXISTS %I ON %I', p, t);
            EXECUTE format('ALTER TABLE %I DISABLE ROW LEVEL SECURITY', t);
        END IF;
    END LOOP;
END
$rls_down$;
