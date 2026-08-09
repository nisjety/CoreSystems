-- ============================================================================
-- Phase 1 RLS, follow-up: transitively org-scoped child tables
-- ============================================================================
--
-- WHAT THIS FIXES — a real defect in 20260809120000_org_rls_isolation.sql
--
-- That migration granted `dataplane_app` privileges only inside its loop over
-- the 35 tables that carry an `org_id` column. Every other table got no grant
-- at all, and `ALTER DEFAULT PRIVILEGES` only covers tables created *later*.
-- So the moment a scoped transaction touched a table without an `org_id` — even
-- just as the join partner of a protected one — it failed outright:
--
--     ERROR:  permission denied for table wiki_page_versions
--     ERROR:  permission denied for table retrieval_candidates
--
-- Both confirmed by direct execution against the live database, not inferred.
-- This surfaced while adopting the scope helper in `retrieval-engine-rs`:
-- `search/wiki.rs::wiki_search` joins `wiki_page_versions`, and
-- `trace/mod.rs` reads/writes `retrieval_candidates`. Scoping those without
-- this migration would have taken down the entire wiki retrieval arm at
-- runtime while every test still passed, because the tests are DB-gated.
--
-- WHY A GRANT ALONE WOULD BE THE WRONG FIX
--
-- These three tables have no `org_id`, but they are not global — each is a
-- child of a protected parent by foreign key, so their rows belong to exactly
-- one org transitively:
--
--     retrieval_candidates.trace_id    -> retrieval_runs.trace_id
--     wiki_page_versions.page_id       -> wiki_pages.page_id
--     chunk_lineage.document_id        -> documents.document_id
--
-- Granting access without a policy would leave a scoped role able to read
-- every org's candidates/versions/lineage by guessing a child key — a hole
-- exactly where this whole phase is meant to close one. So each gets RLS with
-- a policy that derives the org from its parent.
--
-- The parent lookup inside the policy is itself subject to the parent's own
-- policy (same role, same transaction), so the check is enforced twice over:
-- the EXISTS can only match a parent row this org is already allowed to see.
-- There is no recursion risk — the parent policies reference only
-- `current_setting`, never back to these children.
--
-- Performance: every FK column below is indexed (`idx_rc_trace`,
-- `idx_wpv_page`, `idx_cl_document` — verified on the live database), so the
-- EXISTS is an index probe per row rather than a scan.
--
-- `schema_migrations` is deliberately excluded: it is genuine global
-- infrastructure with no tenant meaning, and the migrator that reads and
-- writes it runs as the superuser connection, never as `dataplane_app`.
-- ============================================================================

DO $child_rls$
DECLARE
    t          text;
    child_key  text;
    parent     text;
    parent_key text;
    pol        text;
    -- child table, child FK column, parent table, parent key column
    child_tables text[][] := ARRAY[
        ['retrieval_candidates', 'trace_id',    'retrieval_runs', 'trace_id'],
        ['wiki_page_versions',   'page_id',     'wiki_pages',     'page_id'],
        ['chunk_lineage',        'document_id', 'documents',      'document_id']
    ];
    i int;
BEGIN
    FOR i IN 1 .. array_length(child_tables, 1) LOOP
        t          := child_tables[i][1];
        child_key  := child_tables[i][2];
        parent     := child_tables[i][3];
        parent_key := child_tables[i][4];

        IF NOT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = t AND table_type = 'BASE TABLE'
        ) THEN
            RAISE NOTICE 'dpv2 child RLS: table % absent, skipping', t;
            CONTINUE;
        END IF;

        -- The parent must exist and must itself be policy-protected, or this
        -- child's policy would silently authorize everything.
        IF NOT EXISTS (
            SELECT 1 FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname = 'public' AND c.relname = parent AND c.relrowsecurity
        ) THEN
            RAISE EXCEPTION
                'dpv2 child RLS: parent %.% is missing or has RLS disabled; refusing to create a policy that would depend on it',
                'public', parent;
        END IF;

        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON public.%I TO dataplane_app', t);
        EXECUTE format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY', t);

        FOR pol IN
            SELECT policyname FROM pg_policies
            WHERE schemaname = 'public' AND tablename = t
        LOOP
            EXECUTE format('DROP POLICY IF EXISTS %I ON public.%I', pol, t);
        END LOOP;

        EXECUTE format(
            'CREATE POLICY %I ON public.%I '
            || 'USING (EXISTS (SELECT 1 FROM public.%I p '
            || '                WHERE p.%I = public.%I.%I '
            || '                  AND p.org_id = current_setting(''app.current_org'', true))) '
            || 'WITH CHECK (EXISTS (SELECT 1 FROM public.%I p '
            || '                     WHERE p.%I = public.%I.%I '
            || '                       AND p.org_id = current_setting(''app.current_org'', true)))',
            t || '_rls_isolation', t,
            parent, parent_key, t, child_key,
            parent, parent_key, t, child_key
        );

        RAISE NOTICE 'dpv2 child RLS: % isolated via %.%', t, parent, parent_key;
    END LOOP;
END
$child_rls$;
