-- ============================================================================
-- Row-Level Security org isolation for Data Plane v2 (Phase 1 RLS)
-- ============================================================================
--
-- Closes the "Phase 1 — RLS deferred" item in
-- `docs/sovereign-rag-phased-plan.md`, and is the prerequisite for D-A's
-- account-level data-access grants: those grants widen an org's visibility, and
-- you cannot meaningfully widen a boundary the database does not enforce in the
-- first place. Ported from org-core's proven design (Control Plane migrations
-- 009 + 013), which was audited and empirically verified against its live
-- database on 2026-08-09.
--
-- WHY THIS IS SAFE TO ENABLE ON EVERY TABLE AT ONCE
-- -------------------------------------------------
-- Every DPv2 service connects as `dataplane`, which is SUPERUSER and owns these
-- tables — and PostgreSQL superusers/owners with BYPASSRLS bypass RLS
-- unconditionally, regardless of policy content. So enabling RLS here changes
-- NOTHING for existing code: every current query keeps running exactly as it
-- does today.
--
-- Enforcement is opt-in per transaction. `pg-org-scope-rs` (Rust) and the Go
-- equivalent run, inside one transaction:
--
--     SELECT set_config('app.current_org', '<org_id>', true);  -- SET LOCAL GUC
--     SET LOCAL ROLE dataplane_app;                            -- drop superuser
--
-- For the duration of that transaction the effective role is the NOLOGIN,
-- NOSUPERUSER, NOBYPASSRLS role created below, so the policies actually apply.
-- The role reverts on COMMIT/ROLLBACK, so a pooled connection is never left
-- de-privileged. `dataplane_app` has no password and cannot log in — it is only
-- ever reached via SET ROLE from the already-authenticated superuser
-- connection, adding zero secret-management surface.
--
-- This inertness is the entire reason all 35 org-scoped tables can be enabled
-- in one migration rather than tier-by-tier: the risk lives in ADOPTION (which
-- code paths get wrapped), not in enablement, and adoption is per-service and
-- reviewable one call site at a time.
--
-- STRICT / FAIL-CLOSED FROM DAY ONE
-- ---------------------------------
-- org-core shipped a fail-open policy first (`OR current_setting(...) = ''`)
-- and tightened it later, because at the time it feared policies would apply to
-- its live connection. That fear does not apply here — see above — so there is
-- no incremental-rollout benefit to a fail-open phase, and we go straight to
-- the posture org-core ended at: the GUC must match exactly. An unset GUC makes
-- `org_id = NULL` evaluate to NULL (not true), so a scoped transaction that
-- forgot to set the org sees nothing and writes nothing. That is the intended
-- backstop against a forgotten `WHERE org_id = $1`.
--
-- NULL org_id: `admin_audit_log`, `quickwit_admin_jobs`, and
-- `quickwit_admin_job_audit` allow a NULL `org_id`, meaning a platform-wide
-- record that belongs to no tenant. Under these policies such a row is
-- invisible AND uninsertable from a scoped transaction — which is correct: it
-- is nobody's tenant row. Their writers (GDPR org-purge, Quickwit admin
-- rebuild) are cross-org by design and MUST keep running on the unscoped
-- superuser connection, exactly like org-core's documented admin/erasure
-- exceptions. Do not wrap those paths in the org-scope helper; if you do, the
-- WITH CHECK will reject the insert and the failure will look mysterious.
-- (All three tables are empty as of this migration, so nothing is stranded.)
--
-- ADDING A TABLE: a new org-scoped table is NOT protected automatically. Add it
-- to the array below in the same change that creates it. The list is explicit
-- rather than discovered (`every table with an org_id column`) so that what is
-- protected is auditable by reading this file, and so that protecting a new
-- table stays a deliberate act.
-- ============================================================================

-- Least-privilege runtime role, reached ONLY via SET LOCAL ROLE (no LOGIN).
DO $role$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'dataplane_app') THEN
        CREATE ROLE dataplane_app NOLOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE;
    END IF;
END
$role$;

GRANT USAGE ON SCHEMA public TO dataplane_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO dataplane_app;
-- Future objects created by the migration owner stay reachable by the role, so
-- a later migration does not silently leave the runtime role unable to write.
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO dataplane_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO dataplane_app;

DO $rls$
DECLARE
    t text;
    pol text;
    -- Every table in `public` carrying an `org_id` column, verified against the
    -- live schema on 2026-08-09 (35 tables, all `org_id text` — no cast needed
    -- in the policy expressions below).
    org_tables text[] := ARRAY[
        'access_audit_log',
        'admin_audit_log',
        'agent_retrieval_configs',
        'context_pins',
        'cost_events',
        'data_orchestrator_jobs',
        'data_plane_audit_log',
        'documents',
        'documents_outbox',
        'eval_golden_judgments',
        'graph_claims',
        'graph_communities',
        'graph_entities',
        'graph_exports',
        'graph_relationships',
        'graph_text_units',
        'index_deletion_outbox',
        'index_versions',
        'knowledge_units',
        'operating_map_blueprint_suggestions',
        'operating_map_proposals',
        'operating_map_versions',
        'operating_maps',
        'org_quotas',
        'org_versions',
        'quality_eval_runs',
        'quickwit_admin_job_audit',
        'quickwit_admin_jobs',
        'retrieval_runs',
        'source_objects',
        'wiki_event_outbox',
        'wiki_maintenance_logs',
        'wiki_pages',
        'wiki_proposals',
        'wiki_source_logs'
    ];
    i int;
BEGIN
    FOR i IN 1 .. array_length(org_tables, 1) LOOP
        t := org_tables[i];

        IF NOT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = t AND table_type = 'BASE TABLE'
        ) THEN
            RAISE NOTICE 'dpv2 RLS: table % absent, skipping', t;
            CONTINUE;
        END IF;

        -- Fail loudly rather than creating a policy that silently references a
        -- missing column: a typo here would otherwise produce a table that
        -- looks protected but errors only at query time.
        IF NOT EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = t AND column_name = 'org_id'
        ) THEN
            RAISE EXCEPTION 'dpv2 RLS: table % has no org_id column', t;
        END IF;

        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON public.%I TO dataplane_app', t);
        EXECUTE format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY', t);

        -- Drop every pre-existing policy so exactly one canonical isolation
        -- policy remains and re-running this file is idempotent.
        FOR pol IN
            SELECT policyname FROM pg_policies
            WHERE schemaname = 'public' AND tablename = t
        LOOP
            EXECUTE format('DROP POLICY IF EXISTS %I ON public.%I', pol, t);
        END LOOP;

        EXECUTE format(
            'CREATE POLICY %I ON public.%I '
            || 'USING (org_id = current_setting(''app.current_org'', true)) '
            || 'WITH CHECK (org_id = current_setting(''app.current_org'', true))',
            t || '_rls_isolation', t
        );
    END LOOP;
END
$rls$;
