-- Read-only verification of CR-T6 (20261003130000_context_provenance) on the live
-- Data Plane database. Run it after the Data stage has applied the migration:
--
--   docker exec -i data-plane-v2-postgres-1 psql -U dataplane -d dataplane -X -A \
--     < "apps/Data Plane v2/tests/contract/postgres/live_verify.sql"
--
-- Every row of the first table must say PASS. It reads only: the whole script runs in
-- a READ ONLY transaction that is rolled back, so it cannot change anything.
--
-- The three expected_* numbers describe the live corpus when the migration was
-- written (207 units, 206 of which carry a context, 1 raw). If the corpus has changed
-- since, pass the current figures: -v expected_total=N -v expected_legacy=N
-- -v expected_pending=N. They are what the migration must produce from that corpus,
-- not constants of the schema.
--
-- "Right after applying" checks (status_unknown and updated_at) hold only until the
-- first write by code that predates provenance; after that, status_unknown counts
-- those rows until context_provenance_backfill() is run again (CR-T8 deploy).

\if :{?expected_total}
\else
\set expected_total 207
\endif
\if :{?expected_legacy}
\else
\set expected_legacy 206
\endif
\if :{?expected_pending}
\else
\set expected_pending 1
\endif

BEGIN READ ONLY;

WITH checks(check_name, expected, actual) AS (
  VALUES
    ('migration is recorded', '1',
       (SELECT count(*)::text FROM schema_migrations WHERE version = '20261003130000')),
    ('eleven provenance columns on knowledge_units', '11',
       (SELECT count(*)::text FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = 'knowledge_units'
           AND column_name IN ('context_status', 'context_model', 'context_provider', 'context_residency',
                               'context_input_hash', 'embedded_context_hash', 'context_generated_at',
                               'context_attempts', 'context_next_attempt_at', 'context_error_class',
                               'context_lease_until'))),
    ('seven named CHECK constraints on the provenance columns', '7',
       (SELECT count(*)::text FROM pg_constraint
         WHERE conrelid = 'public.knowledge_units'::regclass
           AND (conname LIKE 'knowledge_units\_context\_%\_chk'
                OR conname = 'knowledge_units_embedded_context_hash_chk'))),
    ('work queue index exists', '1',
       (SELECT count(*)::text FROM pg_indexes WHERE schemaname = 'public' AND indexname = 'idx_ku_context_work')),
    ('context_text_hash is STABLE (s)', 's',
       (SELECT provolatile::text FROM pg_proc WHERE oid = 'public.context_text_hash(text)'::regprocedure)),
    ('coverage view is security_invoker', '{security_invoker=true}',
       (SELECT coalesce(reloptions::text, '') FROM pg_class WHERE oid = 'public.v_context_coverage'::regclass)),
    ('org_processing_policy has row-level security', 'true',
       (SELECT relrowsecurity::text FROM pg_class WHERE oid = 'public.org_processing_policy'::regclass)),
    ('org_processing_policy has exactly one policy', '1',
       (SELECT count(*)::text FROM pg_policy WHERE polrelid = 'public.org_processing_policy'::regclass)),
    ('runtime role can read the coverage view', 'true',
       (SELECT has_table_privilege('dataplane_app', 'public.v_context_coverage', 'SELECT')::text)),
    ('runtime role cannot write the coverage view', 'false',
       (SELECT (has_table_privilege('dataplane_app', 'public.v_context_coverage', 'INSERT')
             OR has_table_privilege('dataplane_app', 'public.v_context_coverage', 'UPDATE')
             OR has_table_privilege('dataplane_app', 'public.v_context_coverage', 'DELETE'))::text)),
    ('runtime role cannot run the backfill', 'false',
       (SELECT has_function_privilege('dataplane_app', 'public.context_provenance_backfill(text)', 'EXECUTE')::text)),
    ('updated_at trigger is enabled again (O)', 'O',
       (SELECT tgenabled::text FROM pg_trigger WHERE tgrelid = 'public.knowledge_units'::regclass
           AND tgname = 'set_knowledge_units_updated_at')),
    ('unit row count unchanged', :'expected_total',
       (SELECT count(*)::text FROM knowledge_units)),
    ('units marked legacy', :'expected_legacy',
       (SELECT coalesce(sum(legacy), 0)::text FROM v_context_coverage)),
    ('units marked pending', :'expected_pending',
       (SELECT coalesce(sum(pending), 0)::text FROM v_context_coverage)),
    ('no unit asserted ready by the backfill', '0',
       (SELECT count(*)::text FROM knowledge_units WHERE context_status = 'ready')),
    ('vector_incoherent reads zero (nothing was asserted)', '0',
       (SELECT coalesce(sum(vector_incoherent), 0)::text FROM v_context_coverage)),
    ('no coherence hash, model or provenance was invented', '0',
       (SELECT count(*)::text FROM knowledge_units
         WHERE embedded_context_hash IS NOT NULL OR context_model IS NOT NULL OR context_provider IS NOT NULL
            OR context_residency IS NOT NULL OR context_input_hash IS NOT NULL OR context_generated_at IS NOT NULL
            OR context_error_class IS NOT NULL OR context_lease_until IS NOT NULL OR context_attempts <> 0)),
    ('embedding_model untouched (still NULL on every unit)', '0',
       (SELECT count(*)::text FROM knowledge_units WHERE embedding_model IS NOT NULL)),
    ('eligible = sum of the statuses in every org', '0',
       (SELECT count(*)::text FROM v_context_coverage
         WHERE eligible <> ready + legacy + pending + skipped_zdr + skipped_policy + skipped_budget
                           + failed_transient + failed_permanent + status_unknown)),
    ('right after applying: no eligible unit has an unknown status', '0',
       (SELECT coalesce(sum(status_unknown), 0)::text FROM v_context_coverage)),
    ('right after applying: updated_at was not moved by the backfill', '0',
       (SELECT count(*)::text FROM knowledge_units
         WHERE context_status IS NOT NULL
           AND updated_at >= (SELECT applied_at FROM schema_migrations WHERE version = '20261003130000')))
)
SELECT check_name, expected, actual, CASE WHEN expected = actual THEN 'PASS' ELSE 'FAIL' END AS result
  FROM checks;

-- For the record: the coverage the new view reports.
SELECT * FROM v_context_coverage ORDER BY org_id;

ROLLBACK;
