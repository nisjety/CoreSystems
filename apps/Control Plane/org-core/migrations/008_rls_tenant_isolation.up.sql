-- ============================================================================
-- 008 — Row-Level Security tenant-isolation backstop  (DELIVERED BUT GATED)
-- ============================================================================
--
-- STATUS: NEEDS DB VALIDATION BEFORE ENABLE. This migration is INERT by default.
--
-- WHY GATED
-- ---------
-- org-core's migration runner (internal/database/migrate.go) re-applies every
-- *.up.sql on each boot and has NO schema_migrations ledger. Enabling RLS blind
-- against the existing query set is unsafe: every repository query
-- (repository.go / rbac/repository.go) runs as the service role WITHOUT setting
-- the request-scoped GUC, so a naive "USING (org_id = current_setting(...))"
-- policy would filter out ALL rows and silently break reads/writes.
--
-- This file therefore does NOTHING unless the operator has explicitly opted in
-- by setting a database-level GUC FIRST:
--
--     ALTER DATABASE <org_core_db> SET app.org_rls_enabled = 'true';
--     -- then redeploy org-core so this migration runs with the flag visible.
--
-- Until that flag is set, the DO block below is a no-op and the schema is
-- unchanged — safe to ship now, enable after validation on a real DB.
--
-- ENABLEMENT MODEL (defense-in-depth, NOT a replacement for the gateway/guard)
-- ---------------------------------------------------------------------------
-- The application sets a request-scoped GUC per transaction:
--     SET LOCAL app.current_org = '<org_id>';
-- (see internal/database/database.go: DB.WithOrgScope). Policies below permit a
-- row when EITHER:
--   (a) app.current_org matches the row's org_id  (request is correctly scoped),
--       OR
--   (b) app.current_org is unset/empty  (background jobs, migrations, cron,
--       cross-org admin reads) — i.e. RLS adds a backstop for request paths
--       WITHOUT breaking the existing unsoped query set.
-- This "permit when GUC matches OR GUC unset" shape is what makes it safe to
-- enable incrementally: nothing breaks on day one, and request paths that DO
-- set the GUC gain a hard DB-level org filter.
--
-- A stricter posture (deny when GUC unset) can be adopted LATER, once every
-- request path is confirmed to set app.current_org, by editing the USING/CHECK
-- expressions to drop the "OR current_setting(...) = ''" clause.
--
-- The service role is NOT given BYPASSRLS here: enforcement stays in-policy so
-- the backstop actually applies to the application connection. Superuser /
-- migration connections bypass RLS inherently.
-- ============================================================================

DO $rls$
BEGIN
    -- Gate: only proceed when the operator has opted in at the database level.
    IF COALESCE(current_setting('app.org_rls_enabled', true), '') <> 'true' THEN
        RAISE NOTICE 'org-core RLS migration 008: app.org_rls_enabled is not ''true'' — skipping (inert).';
        RETURN;
    END IF;

    RAISE NOTICE 'org-core RLS migration 008: app.org_rls_enabled=true — enabling RLS policies.';

    -- ---- organizations -----------------------------------------------------
    -- NOTE: this is the ONLY table whose tenant key is the `id` column — for
    -- organizations the primary key IS the org id. Every other table below
    -- keys on `org_id`. Do not copy this policy to a new table without
    -- changing `id` back to `org_id`, or you create a silent isolation hole.
    EXECUTE 'ALTER TABLE organizations ENABLE ROW LEVEL SECURITY';
    EXECUTE 'DROP POLICY IF EXISTS org_isolation ON organizations';
    EXECUTE $p$
        CREATE POLICY org_isolation ON organizations
        USING (
            id = current_setting('app.current_org', true)
            OR COALESCE(current_setting('app.current_org', true), '') = ''
        )
        WITH CHECK (
            id = current_setting('app.current_org', true)
            OR COALESCE(current_setting('app.current_org', true), '') = ''
        )
    $p$;

    -- ---- org_entitlements --------------------------------------------------
    EXECUTE 'ALTER TABLE org_entitlements ENABLE ROW LEVEL SECURITY';
    EXECUTE 'DROP POLICY IF EXISTS org_ent_isolation ON org_entitlements';
    EXECUTE $p$
        CREATE POLICY org_ent_isolation ON org_entitlements
        USING (
            org_id = current_setting('app.current_org', true)
            OR COALESCE(current_setting('app.current_org', true), '') = ''
        )
        WITH CHECK (
            org_id = current_setting('app.current_org', true)
            OR COALESCE(current_setting('app.current_org', true), '') = ''
        )
    $p$;

    -- ---- organization_members ---------------------------------------------
    EXECUTE 'ALTER TABLE organization_members ENABLE ROW LEVEL SECURITY';
    EXECUTE 'DROP POLICY IF EXISTS org_members_isolation ON organization_members';
    EXECUTE $p$
        CREATE POLICY org_members_isolation ON organization_members
        USING (
            org_id = current_setting('app.current_org', true)
            OR COALESCE(current_setting('app.current_org', true), '') = ''
        )
        WITH CHECK (
            org_id = current_setting('app.current_org', true)
            OR COALESCE(current_setting('app.current_org', true), '') = ''
        )
    $p$;

    -- ---- org_role_mappings (RBAC) -----------------------------------------
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'org_role_mappings') THEN
        EXECUTE 'ALTER TABLE org_role_mappings ENABLE ROW LEVEL SECURITY';
        EXECUTE 'DROP POLICY IF EXISTS org_roles_isolation ON org_role_mappings';
        EXECUTE $p$
            CREATE POLICY org_roles_isolation ON org_role_mappings
            USING (
                org_id = current_setting('app.current_org', true)
                OR COALESCE(current_setting('app.current_org', true), '') = ''
            )
            WITH CHECK (
                org_id = current_setting('app.current_org', true)
                OR COALESCE(current_setting('app.current_org', true), '') = ''
            )
        $p$;
    END IF;

    -- ---- org_tenant_links --------------------------------------------------
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'org_tenant_links') THEN
        EXECUTE 'ALTER TABLE org_tenant_links ENABLE ROW LEVEL SECURITY';
        EXECUTE 'DROP POLICY IF EXISTS org_tenant_links_isolation ON org_tenant_links';
        EXECUTE $p$
            CREATE POLICY org_tenant_links_isolation ON org_tenant_links
            USING (
                org_id = current_setting('app.current_org', true)
                OR COALESCE(current_setting('app.current_org', true), '') = ''
            )
            WITH CHECK (
                org_id = current_setting('app.current_org', true)
                OR COALESCE(current_setting('app.current_org', true), '') = ''
            )
        $p$;
    END IF;

    -- ---- org_onboarding_states --------------------------------------------
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = 'org_onboarding_states') THEN
        EXECUTE 'ALTER TABLE org_onboarding_states ENABLE ROW LEVEL SECURITY';
        EXECUTE 'DROP POLICY IF EXISTS org_onboarding_isolation ON org_onboarding_states';
        EXECUTE $p$
            CREATE POLICY org_onboarding_isolation ON org_onboarding_states
            USING (
                org_id = current_setting('app.current_org', true)
                OR COALESCE(current_setting('app.current_org', true), '') = ''
            )
            WITH CHECK (
                org_id = current_setting('app.current_org', true)
                OR COALESCE(current_setting('app.current_org', true), '') = ''
            )
        $p$;
    END IF;
END
$rls$;
