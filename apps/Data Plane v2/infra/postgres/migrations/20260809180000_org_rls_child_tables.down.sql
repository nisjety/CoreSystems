-- Reverse the transitively-org-scoped child-table isolation: drop the
-- parent-derived policies, disable RLS, and revoke the runtime role's access.
-- The role itself is dropped by 20260809120000_org_rls_isolation.down.sql,
-- which must run after this one (the migrator applies downs in reverse order).

DO $child_rls_down$
DECLARE
    t   text;
    pol text;
    child_tables text[] := ARRAY[
        'retrieval_candidates',
        'wiki_page_versions',
        'chunk_lineage'
    ];
    i int;
BEGIN
    FOR i IN 1 .. array_length(child_tables, 1) LOOP
        t := child_tables[i];

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
$child_rls_down$;
