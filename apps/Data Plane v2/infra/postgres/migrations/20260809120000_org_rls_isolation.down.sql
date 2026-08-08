-- Reverse the Phase 1 RLS org isolation: drop the isolation policies, disable
-- RLS, and remove the runtime role.
--
-- Ordering matters: the role cannot be dropped while it still holds grants or
-- is referenced by a policy, so revoke/drop everything that depends on it
-- first. Dropping the role is safe precisely because nothing logs in as it —
-- it is only ever reached via SET LOCAL ROLE from an already-authenticated
-- connection, so no credential or connection string references it.

DO $rls_down$
DECLARE
    t text;
    pol text;
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
            CONTINUE;
        END IF;

        FOR pol IN
            SELECT policyname FROM pg_policies
            WHERE schemaname = 'public' AND tablename = t
        LOOP
            EXECUTE format('DROP POLICY IF EXISTS %I ON public.%I', pol, t);
        END LOOP;

        EXECUTE format('ALTER TABLE public.%I DISABLE ROW LEVEL SECURITY', t);
        EXECUTE format('REVOKE ALL ON public.%I FROM dataplane_app', t);
    END LOOP;
END
$rls_down$;

DO $role_down$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'dataplane_app') THEN
        ALTER DEFAULT PRIVILEGES IN SCHEMA public
            REVOKE SELECT, INSERT, UPDATE, DELETE ON TABLES FROM dataplane_app;
        ALTER DEFAULT PRIVILEGES IN SCHEMA public
            REVOKE USAGE, SELECT ON SEQUENCES FROM dataplane_app;
        REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM dataplane_app;
        REVOKE ALL ON ALL TABLES IN SCHEMA public FROM dataplane_app;
        REVOKE USAGE ON SCHEMA public FROM dataplane_app;
        DROP ROLE dataplane_app;
    END IF;
END
$role_down$;
