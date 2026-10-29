-- Rollback of 009: disable RLS on the org-scoped tables, drop their isolation
-- policies, revoke the runtime role's privileges, and drop the role. Idempotent.

DO $rls_down$
DECLARE
    t   text;
    pol text;
    org_tables text[] := ARRAY[
        'organizations', 'org_entitlements', 'organization_members',
        'org_tenant_links', 'org_onboarding_states', 'org_role_mappings'
    ];
    i int;
BEGIN
    FOR i IN 1 .. array_length(org_tables, 1) LOOP
        t := org_tables[i];

        IF NOT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = t
        ) THEN
            CONTINUE;
        END IF;

        FOR pol IN
            SELECT policyname FROM pg_policies
            WHERE schemaname = 'public' AND tablename = t
        LOOP
            EXECUTE format('DROP POLICY IF EXISTS %I ON %I', pol, t);
        END LOOP;

        EXECUTE format('ALTER TABLE %I DISABLE ROW LEVEL SECURITY', t);
        EXECUTE format('REVOKE ALL ON %I FROM org_core_app', t);
    END LOOP;
END
$rls_down$;

DO $role_down$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'org_core_app') THEN
        EXECUTE 'REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM org_core_app';
        EXECUTE 'REVOKE USAGE ON SCHEMA public FROM org_core_app';
        EXECUTE 'ALTER DEFAULT PRIVILEGES IN SCHEMA public '
             || 'REVOKE SELECT, INSERT, UPDATE, DELETE ON TABLES FROM org_core_app';
        EXECUTE 'ALTER DEFAULT PRIVILEGES IN SCHEMA public '
             || 'REVOKE USAGE, SELECT ON SEQUENCES FROM org_core_app';
        EXECUTE 'DROP ROLE org_core_app';
    END IF;
END
$role_down$;
