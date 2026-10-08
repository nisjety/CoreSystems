package postgrescontract

import (
	"fmt"
	"strings"
	"sync"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Everything in this file runs against a real Postgres 16: the real init.sql, every real
// migration applied by the real tools/migrator, and row-level security exercised through the
// real orgscope helper as the real dataplane_app role (the services connect as a superuser,
// so RLS is inert on any other path). Without TEST_DATABASE_URL the tests skip locally and
// FAIL under REQUIRE_CONTRACT or CI.

var (
	gesSharedMu sync.Mutex
	gesShared   *scratchDB
)

// graphStateDatabase is the one database, migrated through CR-T12, that the read-mostly tests
// share. Tests keep to their own org ids so they cannot disturb each other.
func graphStateDatabase(t *testing.T) *scratchDB {
	t.Helper()
	gesSharedMu.Lock()
	defer gesSharedMu.Unlock()
	if gesShared != nil {
		return gesShared
	}
	db := createScratch(t, false)
	migrate(t, db, "up", "--to", gesVersion)
	gesShared = db
	return db
}

// graphChunk is a knowledge unit of an existing document.
func graphChunk(t *testing.T, pool *pgxpool.Pool, org, doc, id string) {
	t.Helper()
	insertUnit(t, pool, unitSpec{id: id, doc: doc, org: org}, true)
}

func tryExec(t *testing.T, pool *pgxpool.Pool, sql string) error {
	t.Helper()
	_, err := pool.Exec(t.Context(), sql)
	return err
}

// up -> down -> up converges to the same schema, and what the migration backfilled is exactly
// what the graph already held: ready at the legacy version, with the counts of what is mapped
// to the chunk, for chunks whose organization agrees with their mappings, and nothing else.
func TestGraphExtractionStateLifecycleAndBackfill(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)

	migrate(t, db, "up", "--to", cevVersion)
	before := catalog(t, pool)
	if hasEntry(before, "tbl:graph_extraction_state") || hasEntry(before, "view:v_graph_extraction_coverage") {
		t.Fatal("the CR-T12 objects exist before CR-T12 was applied: the test database is not the baseline it claims to be")
	}

	// A graph as the live one is: chunk 1 produced two entities, a relationship and a claim;
	// chunk 2 produced a claim; chunk 3 was tried and found nothing (so it has no rows, which
	// is all the database can say); chunk 4 belongs to org B but one mapping names org A.
	const orgA, orgB = "org-ges-a", "org-ges-b"
	insertDocument(t, pool, "d-ges-a", orgA, "internal", false)
	insertDocument(t, pool, "d-ges-b", orgB, "internal", false)
	for _, u := range []unitSpec{
		{id: "k-ges-1", doc: "d-ges-a", org: orgA},
		{id: "k-ges-2", doc: "d-ges-a", org: orgA},
		{id: "k-ges-3", doc: "d-ges-a", org: orgA},
		{id: "k-ges-4", doc: "d-ges-b", org: orgB},
	} {
		insertUnit(t, pool, u, false)
	}
	execSQL(t, pool, `INSERT INTO graph_entities (entity_id, org_id, entity_type, entity_text, confidence, provenance) VALUES
		('e-ges-1', 'org-ges-a', 'Organization', 'Aquatiq', 0.9, 'extracted'),
		('e-ges-2', 'org-ges-a', 'Concept', 'Hygiene', 0.8, 'extracted'),
		('e-ges-3', 'org-ges-b', 'Concept', 'Other', 0.7, 'extracted')`)
	execSQL(t, pool, `INSERT INTO graph_relationships (rel_id, org_id, entity_a_id, entity_b_id, relation_type, confidence, provenance) VALUES
		('r-ges-1', 'org-ges-a', 'e-ges-1', 'e-ges-2', 'works_on', 0.8, 'extracted')`)
	execSQL(t, pool, `INSERT INTO graph_claims (claim_id, org_id, claim_text, confidence, provenance) VALUES
		('c-ges-1', 'org-ges-a', 'Aquatiq sells hygiene', 0.8, 'extracted'),
		('c-ges-2', 'org-ges-a', 'Another claim', 0.8, 'extracted')`)
	execSQL(t, pool, `INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id) VALUES
		('org-ges-a', 'k-ges-1', 'e-ges-1', NULL, NULL),
		('org-ges-a', 'k-ges-1', 'e-ges-2', NULL, NULL),
		('org-ges-a', 'k-ges-1', NULL, 'r-ges-1', NULL),
		('org-ges-a', 'k-ges-1', NULL, NULL, 'c-ges-1'),
		('org-ges-a', 'k-ges-2', NULL, NULL, 'c-ges-2'),
		('org-ges-a', 'k-ges-4', 'e-ges-1', NULL, NULL)`)

	out := migrate(t, db, "up", "--to", gesVersion)
	if !strings.Contains(out, gesVersion) {
		t.Fatalf("migrator did not report applying %s:\n%s", gesVersion, out)
	}
	applied := catalog(t, pool)
	for _, want := range []string{
		"tbl:graph_extraction_state:rls=true:",
		"view:v_graph_extraction_coverage:opts={security_invoker=true}:",
		"idx:idx_ges_work:",
		"idx:idx_ges_status:",
		"col:graph_extraction_state.attempts:integer:NO:0",
		"col:graph_extraction_state.status:text:NO:'pending'::text",
		"col:graph_extraction_state.next_attempt_at:timestamp with time zone:YES:",
		"col:graph_extraction_state.lease_until:timestamp with time zone:YES:",
		"pol:graph_extraction_state.graph_extraction_state_rls_isolation:",
		"trg:graph_extraction_state.set_graph_extraction_state_updated_at:O",
	} {
		if !hasEntry(applied, want) {
			t.Errorf("after CR-T12 the catalog has no entry starting %q", want)
		}
	}
	if t.Failed() {
		t.FailNow()
	}

	// What the backfill recorded: chunk 1 (2 entities, 1 relationship, 1 claim) and chunk 2
	// (1 claim) are ready at the legacy version with no model; chunk 3 and the mismatched
	// mapping of chunk 4 are not recorded.
	rows := queryLines(t, pool, `SELECT knowledge_id || '/' || status || '/' || prompt_version || '/' || coalesce(model, '<NULL>') || '/' ||
		entity_count || '/' || relationship_count || '/' || claim_count || '/' || attempts
		FROM graph_extraction_state WHERE org_id LIKE 'org-ges-%' ORDER BY knowledge_id`)
	want := []string{
		"k-ges-1/ready/legacy/<NULL>/2/1/1/0",
		"k-ges-2/ready/legacy/<NULL>/0/0/1/0",
	}
	if strings.Join(rows, "|") != strings.Join(want, "|") {
		t.Errorf("the backfill recorded %v, want %v", rows, want)
	}
	if n := queryInt(t, pool, `SELECT count(*) FROM graph_extraction_state WHERE extracted_at IS NULL OR extracted_at > now()`); n != 0 {
		t.Errorf("%d backfilled rows have no extraction time or one in the future", n)
	}

	// Down: the schema returns to exactly what it was; the graph itself is untouched.
	migrate(t, db, "down", "--to", cevVersion)
	assertSameCatalog(t, "after rolling CR-T12 back", before, catalog(t, pool))
	if n := queryInt(t, pool, `SELECT count(*) FROM graph_text_units`); n != 6 {
		t.Errorf("the rollback changed the graph: %d mappings, want 6", n)
	}

	// Up again converges to the very same schema and records the very same rows.
	migrate(t, db, "up", "--to", gesVersion)
	assertSameCatalog(t, "after applying CR-T12 a second time", applied, catalog(t, pool))
	again := queryLines(t, pool, `SELECT knowledge_id || '/' || status || '/' || prompt_version FROM graph_extraction_state WHERE org_id LIKE 'org-ges-%' ORDER BY knowledge_id`)
	if strings.Join(again, "|") != "k-ges-1/ready/legacy|k-ges-2/ready/legacy" {
		t.Errorf("after the second application the table holds %v", again)
	}

	// The forward file is idempotent on its own, and it never rewrites what graph-index has
	// recorded since: a re-run must not move a row the service owns.
	execSQL(t, pool, `UPDATE graph_extraction_state SET status = 'ready', prompt_version = 'graph-v1', model = 'gpt-4o-mini', provider = 'azure-openai',
		entity_count = 5, attempts = 1 WHERE knowledge_id = 'k-ges-1'`)
	execSQL(t, pool, readSQL(t, gesUp))
	assertSameCatalog(t, "after re-running the forward file", applied, catalog(t, pool))
	if got := queryString(t, pool, `SELECT prompt_version || '/' || model || '/' || entity_count || '/' || attempts FROM graph_extraction_state WHERE knowledge_id = 'k-ges-1'`); got != "graph-v1/gpt-4o-mini/5/1" {
		t.Errorf("re-running the migration rewrote a row graph-index owns: %s", got)
	}
}

// A migration that backfills must abort rather than ship a partial result.
func TestGraphExtractionStatePostConditionAbortsWhenAChunkWithGraphRowsHasNoState(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)
	migrate(t, db, "up", "--to", gesVersion)

	insertDocument(t, pool, "d-post", "org-ges-post", "internal", false)
	insertUnit(t, pool, unitSpec{id: "k-post", doc: "d-post", org: "org-ges-post"}, true)
	execSQL(t, pool, `INSERT INTO graph_entities (entity_id, org_id, entity_type, entity_text, provenance) VALUES ('e-post', 'org-ges-post', 'Concept', 'X', 'extracted')`)
	execSQL(t, pool, `INSERT INTO graph_text_units (org_id, knowledge_id, entity_id) VALUES ('org-ges-post', 'k-post', 'e-post')`)

	// The state a later insert never wrote: running the forward file's post-condition alone
	// (the block after the backfill) must raise. Re-run the file with the backfill's INSERT
	// removed to prove the block is what stops it.
	sql := readSQL(t, gesUp)
	start := strings.Index(sql, "INSERT INTO graph_extraction_state")
	end := strings.Index(sql[start:], ";") + start + 1
	err := tryExec(t, pool, sql[:start]+sql[end:])
	if err == nil {
		t.Fatal("the post-condition accepted a chunk with graph rows and no state")
	}
	if !strings.Contains(err.Error(), "left 1 chunk(s) with graph rows and no state") {
		t.Fatalf("the migration failed for another reason: %v", err)
	}
}

func TestGraphExtractionStateConstraints(t *testing.T) {
	pool := openPool(t, graphStateDatabase(t))
	insertDocument(t, pool, "d-chk", "org-ges-chk", "internal", false)
	for i := 1; i <= 12; i++ {
		graphChunk(t, pool, "org-ges-chk", "d-chk", fmt.Sprintf("k-chk-%d", i))
	}

	insert := func(n int, columns, values string) error {
		return tryExec(t, pool, fmt.Sprintf(
			`INSERT INTO graph_extraction_state (org_id, knowledge_id, %s) VALUES ('org-ges-chk', 'k-chk-%d', %s)`, columns, n, values))
	}

	t.Run("a bare pending row is valid and defaults are what the service relies on", func(t *testing.T) {
		if err := insert(1, "status", "'pending'"); err != nil {
			t.Fatalf("a pending row was refused: %v", err)
		}
		if got := queryString(t, pool, `SELECT attempts || '/' || coalesce(next_attempt_at::text, '<NULL>') || '/' || coalesce(lease_until::text, '<NULL>') FROM graph_extraction_state WHERE knowledge_id = 'k-chk-1'`); got != "0/<NULL>/<NULL>" {
			t.Errorf("a new row starts at %s, want 0/<NULL>/<NULL>", got)
		}
	})
	t.Run("a status the vocabulary does not know is refused", func(t *testing.T) {
		expectPgError(t, insert(2, "status", "'done'"), "23514", "graph_extraction_state_status_chk")
	})
	t.Run("an error class the vocabulary does not know is refused", func(t *testing.T) {
		expectPgError(t, insert(2, "status, error_class", "'failed_transient', 'the provider said: leaked document text'"), "23514", "graph_extraction_state_error_class_chk")
	})
	t.Run("a failure and a zero-retention skip always say why", func(t *testing.T) {
		for _, status := range []string{"failed_transient", "failed_permanent", "skipped_zdr"} {
			expectPgError(t, insert(3, "status", q(status)), "23514", "graph_extraction_state_failure_class_chk")
		}
		for _, status := range []string{"skipped_policy", "skipped_visibility", "skipped_budget", "pending"} {
			execSQL(t, pool, `DELETE FROM graph_extraction_state WHERE knowledge_id = 'k-chk-3'`)
			if err := insert(3, "status", q(status)); err != nil {
				t.Errorf("%s without a class was refused: %v", status, err)
			}
		}
	})
	t.Run("a completed extraction is stamped", func(t *testing.T) {
		expectPgError(t, insert(4, "status", "'ready'"), "23514", "graph_extraction_state_stamp_chk")
		expectPgError(t, insert(4, "status, prompt_version, extracted_at, entity_count, relationship_count, claim_count",
			"'ready', 'graph-v1', now(), 1, 0, 0"), "23514", "graph_extraction_state_stamp_chk") // no model, not legacy
		if err := insert(4, "status, prompt_version, model, extracted_at, entity_count, relationship_count, claim_count",
			"'ready', 'graph-v1', 'gpt-4o-mini', now(), 1, 0, 0"); err != nil {
			t.Errorf("a stamped ready row was refused: %v", err)
		}
		if err := insert(5, "status, prompt_version, extracted_at, entity_count, relationship_count, claim_count",
			"'ready', 'legacy', now(), 1, 0, 0"); err != nil {
			t.Errorf("a legacy row without a model was refused: %v", err)
		}
	})
	t.Run("ready stored something and empty stored nothing", func(t *testing.T) {
		const stamp = "'graph-v1', 'gpt-4o-mini', now()"
		expectPgError(t, insert(6, "status, prompt_version, model, extracted_at, entity_count, relationship_count, claim_count",
			"'ready', "+stamp+", 0, 0, 0"), "23514", "graph_extraction_state_yield_chk")
		expectPgError(t, insert(6, "status, prompt_version, model, extracted_at, entity_count, relationship_count, claim_count",
			"'empty', "+stamp+", 0, 1, 0"), "23514", "graph_extraction_state_yield_chk")
		if err := insert(6, "status, prompt_version, model, extracted_at, entity_count, relationship_count, claim_count",
			"'empty', "+stamp+", 0, 0, 0"); err != nil {
			t.Errorf("an empty row with zero counts was refused: %v", err)
		}
	})
	t.Run("counters never go negative", func(t *testing.T) {
		expectPgError(t, insert(7, "attempts", "-1"), "23514", "graph_extraction_state_counters_chk")
		expectPgError(t, insert(7, "status, prompt_version, model, extracted_at, entity_count, relationship_count, claim_count",
			"'ready', 'graph-v1', 'm', now(), -1, 2, 1"), "23514", "graph_extraction_state_counters_chk")
	})
	t.Run("a blank organization or prompt version is refused", func(t *testing.T) {
		expectPgError(t, tryExec(t, pool, `INSERT INTO graph_extraction_state (org_id, knowledge_id) VALUES ('  ', 'k-chk-8')`), "23514", "graph_extraction_state_org_chk")
		expectPgError(t, insert(8, "prompt_version", "'  '"), "23514", "graph_extraction_state_prompt_version_chk")
	})
	t.Run("a chunk that does not exist cannot have a state", func(t *testing.T) {
		expectPgError(t, tryExec(t, pool, `INSERT INTO graph_extraction_state (org_id, knowledge_id) VALUES ('org-ges-chk', 'k-nowhere')`), "23503", "")
	})
	t.Run("one row per organization and chunk", func(t *testing.T) {
		expectPgError(t, insert(1, "status", "'pending'"), "23505", "graph_extraction_state_pkey")
	})
	t.Run("updated_at moves with the row", func(t *testing.T) {
		// The trigger overwrites an explicit updated_at, so the proof is that a plain UPDATE
		// moves a row whose updated_at was last set (by the trigger) to now.
		execSQL(t, pool, `ALTER TABLE graph_extraction_state DISABLE TRIGGER set_graph_extraction_state_updated_at`)
		execSQL(t, pool, `UPDATE graph_extraction_state SET updated_at = now() - interval '2 days' WHERE knowledge_id = 'k-chk-1'`)
		execSQL(t, pool, `ALTER TABLE graph_extraction_state ENABLE TRIGGER set_graph_extraction_state_updated_at`)
		execSQL(t, pool, `UPDATE graph_extraction_state SET attempts = 1 WHERE knowledge_id = 'k-chk-1'`)
		if n := queryInt(t, pool, `SELECT count(*) FROM graph_extraction_state WHERE knowledge_id = 'k-chk-1' AND updated_at > now() - interval '1 day'`); n != 1 {
			t.Error("the updated_at trigger did not fire")
		}
	})
}

// A re-chunk deletes the old knowledge units; their state goes with them, like graph_text_units.
func TestGraphExtractionStateCascadesWithItsKnowledgeUnit(t *testing.T) {
	pool := openPool(t, graphStateDatabase(t))
	insertDocument(t, pool, "d-casc", "org-ges-casc", "internal", false)
	graphChunk(t, pool, "org-ges-casc", "d-casc", "k-casc-1")
	graphChunk(t, pool, "org-ges-casc", "d-casc", "k-casc-2")
	execSQL(t, pool, `INSERT INTO graph_extraction_state (org_id, knowledge_id) VALUES ('org-ges-casc', 'k-casc-1'), ('org-ges-casc', 'k-casc-2')`)

	execSQL(t, pool, `DELETE FROM knowledge_units WHERE knowledge_id = 'k-casc-1'`)
	if got := queryLines(t, pool, `SELECT knowledge_id FROM graph_extraction_state WHERE org_id = 'org-ges-casc' ORDER BY 1`); strings.Join(got, ",") != "k-casc-2" {
		t.Errorf("after deleting one unit the state holds %v, want only k-casc-2", got)
	}
	execSQL(t, pool, `DELETE FROM documents WHERE document_id = 'd-casc'`)
	if n := queryInt(t, pool, `SELECT count(*) FROM graph_extraction_state WHERE org_id = 'org-ges-casc'`); n != 0 {
		t.Errorf("%d state rows outlived their document", n)
	}
}

// The work queue is partial over the statuses a worker picks up, so the index stays small
// however many chunks are final.
func TestGraphExtractionStateWorkQueueIndexIsPartialOverTheNonFinalStatuses(t *testing.T) {
	pool := openPool(t, graphStateDatabase(t))
	def := queryString(t, pool, `SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND indexname = 'idx_ges_work'`)
	for _, want := range []string{"(org_id, next_attempt_at)", "'pending'", "'failed_transient'", "'skipped_zdr'", "'skipped_policy'", "'skipped_visibility'", "'skipped_budget'"} {
		if !strings.Contains(def, want) {
			t.Errorf("idx_ges_work does not contain %s: %s", want, def)
		}
	}
	for _, final := range []string{"'ready'", "'empty'", "'failed_permanent'"} {
		if strings.Contains(def, final) {
			t.Errorf("idx_ges_work indexes final status %s: %s", final, def)
		}
	}
}

// ── row-level security, as the runtime role, through the real helper ─────────

func TestGraphExtractionStateIsIsolatedPerOrgForTheRuntimeRole(t *testing.T) {
	pool := openPool(t, graphStateDatabase(t))
	if got := queryString(t, pool, "SELECT rolsuper::text || '/' || rolbypassrls::text || '/' || rolcanlogin::text FROM pg_roles WHERE rolname = 'dataplane_app'"); got != "false/false/false" {
		t.Fatalf("dataplane_app (super/bypassrls/login) = %s, want false/false/false: every isolation test below would pass vacuously", got)
	}

	insertDocument(t, pool, "d-rls-ga", "org-ges-rls-a", "internal", false)
	insertDocument(t, pool, "d-rls-gb", "org-ges-rls-b", "internal", false)
	graphChunk(t, pool, "org-ges-rls-a", "d-rls-ga", "k-rls-ga-1")
	graphChunk(t, pool, "org-ges-rls-a", "d-rls-ga", "k-rls-ga-2")
	graphChunk(t, pool, "org-ges-rls-a", "d-rls-ga", "k-rls-ga-3") // no state
	graphChunk(t, pool, "org-ges-rls-b", "d-rls-gb", "k-rls-gb-1")
	execSQL(t, pool, `INSERT INTO graph_extraction_state (org_id, knowledge_id, status, error_class) VALUES
		('org-ges-rls-a', 'k-rls-ga-1', 'skipped_zdr', 'zdr_org'),
		('org-ges-rls-a', 'k-rls-ga-2', 'pending', NULL),
		('org-ges-rls-b', 'k-rls-gb-1', 'pending', NULL)`)

	const listSQL = "SELECT knowledge_id FROM graph_extraction_state WHERE knowledge_id LIKE 'k-rls-g%' ORDER BY knowledge_id"
	if got := scopedStrings(t, pool, "org-ges-rls-a", listSQL); strings.Join(got, ",") != "k-rls-ga-1,k-rls-ga-2" {
		t.Errorf("org A sees %v", got)
	}
	if got := scopedStrings(t, pool, "org-ges-rls-b", listSQL); strings.Join(got, ",") != "k-rls-gb-1" {
		t.Errorf("org B sees %v", got)
	}
	if got := scopedStrings(t, pool, "org-ges-rls-c", listSQL); len(got) != 0 {
		t.Errorf("an org with no chunks sees %v", got)
	}

	t.Run("org B cannot change, claim or delete org A's row", func(t *testing.T) {
		for _, sql := range []string{
			"UPDATE graph_extraction_state SET status = 'failed_permanent', error_class = 'internal' WHERE org_id = 'org-ges-rls-a'",
			"UPDATE graph_extraction_state SET attempts = attempts + 1, lease_until = now() + interval '5 minutes' WHERE knowledge_id = 'k-rls-ga-2'",
			"DELETE FROM graph_extraction_state WHERE org_id = 'org-ges-rls-a'",
		} {
			if n, err := scopedExec(t, pool, "org-ges-rls-b", sql); err != nil || n != 0 {
				t.Errorf("cross-org statement affected %d rows, err %v; want 0 rows\nSQL: %s", n, err, sql)
			}
		}
		if got := queryString(t, pool, "SELECT status || '/' || attempts FROM graph_extraction_state WHERE knowledge_id = 'k-rls-ga-2'"); got != "pending/0" {
			t.Errorf("org A's row is now %s", got)
		}
	})
	t.Run("org B cannot write a row for another org's chunk or move its own", func(t *testing.T) {
		_, err := scopedExec(t, pool, "org-ges-rls-b", "INSERT INTO graph_extraction_state (org_id, knowledge_id) VALUES ('org-ges-rls-a', 'k-rls-ga-3')")
		expectPgError(t, err, "42501", "")
		_, err = scopedExec(t, pool, "org-ges-rls-b", "UPDATE graph_extraction_state SET org_id = 'org-ges-rls-a' WHERE org_id = 'org-ges-rls-b'")
		expectPgError(t, err, "42501", "")
	})
	t.Run("an org can claim and settle its own chunk", func(t *testing.T) {
		n, err := scopedExec(t, pool, "org-ges-rls-b", "UPDATE graph_extraction_state SET attempts = attempts + 1, lease_until = now() + interval '5 minutes' WHERE knowledge_id = 'k-rls-gb-1'")
		if err != nil || n != 1 {
			t.Fatalf("own-org UPDATE: %d rows, err %v", n, err)
		}
	})
	t.Run("a transaction that forgot to set the org sees nothing", func(t *testing.T) {
		tx, err := pool.Begin(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = tx.Rollback(t.Context()) }()
		if _, err := tx.Exec(t.Context(), "SET LOCAL ROLE dataplane_app"); err != nil {
			t.Fatal(err)
		}
		var n int64
		if err := tx.QueryRow(t.Context(), "SELECT count(*) FROM graph_extraction_state").Scan(&n); err != nil {
			t.Fatal(err)
		}
		if n != 0 {
			t.Errorf("an unscoped runtime-role transaction read %d rows", n)
		}
	})
	t.Run("the policy is the same as every other org table's", func(t *testing.T) {
		const policySQL = `SELECT pg_get_expr(polqual, polrelid) || ' / ' || pg_get_expr(polwithcheck, polrelid) || ' / ' ||
			polcmd::text || ' / ' || polpermissive::text FROM pg_policy WHERE polrelid = '%s'::regclass`
		want := queryString(t, pool, fmt.Sprintf(policySQL, "public.knowledge_units"))
		got := queryString(t, pool, fmt.Sprintf(policySQL, "public.graph_extraction_state"))
		if got != want {
			t.Errorf("graph_extraction_state policy = %s\n   knowledge_units policy = %s", got, want)
		}
		if got := queryString(t, pool, "SELECT relrowsecurity::text FROM pg_class WHERE oid = 'public.graph_extraction_state'::regclass"); got != "true" {
			t.Errorf("row-level security is not enabled on graph_extraction_state: %s", got)
		}
	})
}

func TestGraphCoveragePartitionsChunksAndHonoursRowLevelSecurity(t *testing.T) {
	pool := openPool(t, graphStateDatabase(t))
	const orgA, orgB = "org-ges-cov-a", "org-ges-cov-b"

	insertDocument(t, pool, "d-cov-ok", orgA, "internal", false)
	insertDocument(t, pool, "d-cov-private", orgA, "internal", false)
	insertDocument(t, pool, "d-cov-restricted", orgA, "restricted", false)
	insertDocument(t, pool, "d-cov-deleted", orgA, "internal", true)
	insertDocument(t, pool, "d-cov-b", orgB, "public", false)
	execSQL(t, pool, `UPDATE documents SET visibility = 'private' WHERE document_id = 'd-cov-private'`)
	for _, c := range []struct{ id, doc, org string }{
		{"k-cov-none", "d-cov-ok", orgA},
		{"k-cov-ready", "d-cov-ok", orgA},
		{"k-cov-legacy", "d-cov-ok", orgA},
		{"k-cov-empty", "d-cov-ok", orgA},
		{"k-cov-failed", "d-cov-ok", orgA},
		{"k-cov-private", "d-cov-private", orgA},
		{"k-cov-restricted", "d-cov-restricted", orgA},
		{"k-cov-deleted", "d-cov-deleted", orgA},
		{"k-cov-b-none", "d-cov-b", orgB},
	} {
		graphChunk(t, pool, c.org, c.doc, c.id)
	}
	execSQL(t, pool, `INSERT INTO graph_extraction_state
		(org_id, knowledge_id, status, prompt_version, model, extracted_at, entity_count, relationship_count, claim_count, error_class) VALUES
		('org-ges-cov-a', 'k-cov-ready',   'ready',            'graph-v1', 'gpt-4o-mini', now(), 3, 1, 0, NULL),
		('org-ges-cov-a', 'k-cov-legacy',  'ready',            'legacy',   NULL,          now(), 1, 0, 0, NULL),
		('org-ges-cov-a', 'k-cov-empty',   'empty',            'graph-v1', 'gpt-4o-mini', now(), 0, 0, 0, NULL),
		('org-ges-cov-a', 'k-cov-failed',  'failed_permanent', NULL,       NULL,          NULL,  NULL, NULL, NULL, 'invalid_output'),
		('org-ges-cov-a', 'k-cov-private', 'skipped_visibility', NULL,     NULL,          NULL,  NULL, NULL, NULL, NULL),
		('org-ges-cov-a', 'k-cov-restricted', 'skipped_zdr',   NULL,       NULL,          NULL,  NULL, NULL, NULL, 'zdr_document')`)

	// Superuser: every organization, and only the chunks extraction may look at.
	got := queryLines(t, pool, `SELECT org_id || ':' || chunks || '/' || no_state || '/' || pending || '/' || ready || '/' || ready_legacy || '/' || empty || '/' ||
		skipped_zdr || '/' || skipped_policy || '/' || skipped_visibility || '/' || skipped_budget || '/' || failed_transient || '/' || failed_permanent
		FROM v_graph_extraction_coverage WHERE org_id LIKE 'org-ges-cov-%' ORDER BY org_id`)
	want := []string{
		// 5 chunks of the visible, live, internal document; the private, restricted and deleted ones are out.
		"org-ges-cov-a:5/1/0/2/1/1/0/0/0/0/0/1",
		"org-ges-cov-b:1/1/0/0/0/0/0/0/0/0/0/0",
	}
	if strings.Join(got, "|") != strings.Join(want, "|") {
		t.Errorf("coverage = %v, want %v", got, want)
	}

	// The runtime role sees its own organization's row and nobody else's.
	const viewSQL = "SELECT org_id FROM v_graph_extraction_coverage WHERE org_id LIKE 'org-ges-cov-%' ORDER BY org_id"
	if got := scopedStrings(t, pool, orgA, viewSQL); strings.Join(got, ",") != orgA {
		t.Errorf("org A reads %v through the coverage view", got)
	}
	if got := scopedStrings(t, pool, orgB, viewSQL); strings.Join(got, ",") != orgB {
		t.Errorf("org B reads %v through the coverage view", got)
	}
	// And cannot write through it.
	if _, err := scopedExec(t, pool, orgA, "DELETE FROM v_graph_extraction_coverage"); err == nil {
		t.Error("the runtime role deleted through the coverage view")
	}
}

func TestGraphExtractionStateRuntimeRolePrivilegesAreLeastPrivilege(t *testing.T) {
	pool := openPool(t, graphStateDatabase(t))
	priv := func(relation, privilege string) string {
		return queryString(t, pool, fmt.Sprintf("SELECT has_table_privilege('dataplane_app', 'public.%s', '%s')::text", relation, privilege))
	}
	for _, p := range []string{"SELECT", "INSERT", "UPDATE", "DELETE"} {
		if got := priv("graph_extraction_state", p); got != "true" {
			t.Errorf("dataplane_app lacks %s on graph_extraction_state", p)
		}
	}
	for _, p := range []string{"TRUNCATE", "REFERENCES", "TRIGGER"} {
		if got := priv("graph_extraction_state", p); got != "false" {
			t.Errorf("dataplane_app has %s on graph_extraction_state", p)
		}
	}
	if got := priv("v_graph_extraction_coverage", "SELECT"); got != "true" {
		t.Error("dataplane_app cannot read v_graph_extraction_coverage")
	}
	for _, p := range []string{"INSERT", "UPDATE", "DELETE"} {
		if got := priv("v_graph_extraction_coverage", p); got != "false" {
			t.Errorf("dataplane_app has %s on the read-only view v_graph_extraction_coverage", p)
		}
	}
	if n := queryInt(t, pool, `SELECT count(*) FROM information_schema.role_table_grants WHERE grantee = 'PUBLIC' AND table_schema = 'public' AND table_name = 'graph_extraction_state'`); n != 0 {
		t.Errorf("PUBLIC holds %d privileges on graph_extraction_state", n)
	}
}
