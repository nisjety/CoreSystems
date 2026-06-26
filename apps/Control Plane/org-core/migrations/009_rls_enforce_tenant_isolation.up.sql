-- ============================================================================
-- 009 — Activate Row-Level Security tenant isolation  (Phase 6, ENFORCED)
-- ============================================================================
--
-- Supersedes the gated, inert migration 008. B4 introduced the
-- schema_migrations ledger (internal/database/migrate.go), so RLS can now be
-- enabled deterministically on deploy and runs exactly once — the precondition
-- 008 documented as the reason it could not be enabled blind. This migration is
-- UNGATED: it always enables RLS + policies on the org-scoped tables.
--
-- ENFORCEMENT MODEL — why a SET ROLE, not a connection swap
-- --------------------------------------------------------
-- org-core connects to its database as a role (aquatiq) that is SUPERUSER and
-- owns these tables, and PostgreSQL superusers/owners-with-BYPASSRLS bypass RLS
-- unconditionally. Enabling RLS alone would therefore be inert against the live
-- connection. Rather than introduce a new login role + password (secret-mgmt
-- surface) or repoint the live identity service, enforcement is opt-in per
-- request transaction: DB.WithOrgScope (internal/database/database.go) runs
--
--     SELECT set_config('app.current_org', '<org_id>', true);  -- SET LOCAL GUC
--     SET LOCAL ROLE org_core_app;                             -- drop superuser
--
-- so for the duration of that transaction the effective role is the NOLOGIN,
-- NOSUPERUSER, NOBYPASSRLS role created below and the policies actually apply.
-- After COMMIT/ROLLBACK the role reverts automatically, so the pooled
-- connection is never left de-privileged. org_core_app has NO password and
-- cannot log in — it is only ever reached via SET ROLE from the already
-- authenticated superuser connection, adding zero secret-management surface.
--
-- Unscoped reads (admin list-all, multi-org "orgs for this user", lookup by
-- secondary key, GDPR erasure procs) intentionally run as the superuser
-- connection WITHOUT SET ROLE and see across tenants — that is their job, and
-- they remain gated by the application authorization layer.
--
-- POLICY SHAPE — permit when GUC matches OR GUC unset
-- --------------------------------------------------
-- Each policy permits a row when the request-scoped GUC matches the row's
-- tenant key OR the GUC is unset/empty. The "unset" branch is defense-in-depth
-- that can never break a query that forgot to scope (it simply falls back to the
-- app-layer guard). A stricter "deny when unset" posture can be adopted later by
-- dropping the "OR current_setting(...) = ''" clause, once every scoped path is
-- confirmed to set the GUC.
-- ============================================================================

-- Least-privilege runtime role, reached ONLY via SET LOCAL ROLE (no LOGIN).
DO $role$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'org_core_app') THEN
        CREATE ROLE org_core_app NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
    END IF;
END
$role$;

GRANT USAGE ON SCHEMA public TO org_core_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO org_core_app;
-- Future objects created by the migration owner stay reachable by the role.
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO org_core_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO org_core_app;

-- Per-table: grant the runtime role CRUD, enable RLS, (re)create the isolation
-- policy. Wrapped in IF EXISTS so a fresh database (where a table is created by
-- an earlier migration) and an existing one are both handled, and so the file
-- is safe to re-assert. `organizations` keys on `id` (its PK is the org id);
-- every other table keys on `org_id` — do not copy the `id` policy to an
-- `org_id` table or you open a silent isolation hole.
DO $rls$
DECLARE
    t   text;
    key text;
    pol text;
    org_tables text[][] := ARRAY[
        ['organizations',         'id'],
        ['org_entitlements',      'org_id'],
        ['organization_members',  'org_id'],
        ['org_tenant_links',      'org_id'],
        ['org_onboarding_states', 'org_id'],
        ['org_role_mappings',     'org_id']
    ];
    i int;
BEGIN
    FOR i IN 1 .. array_length(org_tables, 1) LOOP
        t   := org_tables[i][1];
        key := org_tables[i][2];

        IF NOT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = t
        ) THEN
            CONTINUE;
        END IF;

        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON %I TO org_core_app', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);

        -- Drop every pre-existing policy on this table (the inert 008 policy, a
        -- validation prototype, or a prior run of this migration) so exactly one
        -- canonical isolation policy remains.
        FOR pol IN
            SELECT policyname FROM pg_policies
            WHERE schemaname = 'public' AND tablename = t
        LOOP
            EXECUTE format('DROP POLICY IF EXISTS %I ON %I', pol, t);
        END LOOP;

        EXECUTE format(
            'CREATE POLICY %I ON %I '
            || 'USING (%I = current_setting(''app.current_org'', true) '
            || '       OR COALESCE(current_setting(''app.current_org'', true), '''') = '''') '
            || 'WITH CHECK (%I = current_setting(''app.current_org'', true) '
            || '       OR COALESCE(current_setting(''app.current_org'', true), '''') = '''')',
            t || '_rls_isolation', t, key, key
        );

        RAISE NOTICE 'org-core RLS 009: enabled on % (key %)', t, key;
    END LOOP;
END
$rls$;
