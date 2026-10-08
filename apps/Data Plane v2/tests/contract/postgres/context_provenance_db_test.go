package postgrescontract

import (
	"context"
	"fmt"
	"regexp"
	"sort"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// Everything in this file runs against a real Postgres 16: the real init.sql,
// every real migration applied in order by the real tools/migrator binary, and
// row-level security exercised through the real orgscope helper as the real
// dataplane_app role. Nothing here is a stub of a rule the production path
// applies. Without TEST_DATABASE_URL the tests skip locally and FAIL under
// REQUIRE_CONTRACT or CI (see requireDatabase).

// ── catalog snapshots ────────────────────────────────────────────────────────

// catalogSQL describes the schema's shape: columns, constraints, indexes,
// functions (with volatility, strictness and per-function settings), tables
// (row-level security and privileges), views (options and privileges),
// policies, triggers and comments. Two snapshots are equal exactly when nothing
// observable about the schema differs.
const catalogSQL = `
SELECT line FROM (
  SELECT 'col:' || table_name || '.' || column_name || ':' || data_type || ':' || is_nullable || ':' || coalesce(column_default, '') AS line
    FROM information_schema.columns WHERE table_schema = 'public'
  UNION ALL
  SELECT 'con:' || conrelid::regclass::text || '.' || conname || ':' || pg_get_constraintdef(oid)
    FROM pg_constraint WHERE connamespace = 'public'::regnamespace
  UNION ALL
  SELECT 'idx:' || indexname || ':' || indexdef FROM pg_indexes WHERE schemaname = 'public'
  UNION ALL
  SELECT 'fn:' || p.oid::regprocedure::text || ':' || p.provolatile::text || ':' || p.proisstrict::text || ':' || coalesce(p.proconfig::text, '')
    FROM pg_proc p
   WHERE p.pronamespace = 'public'::regnamespace
     AND NOT EXISTS (SELECT 1 FROM pg_depend d WHERE d.objid = p.oid AND d.deptype = 'e')
  UNION ALL
  SELECT 'tbl:' || c.relname || ':rls=' || c.relrowsecurity::text || ':acl=' || coalesce(c.relacl::text, '')
    FROM pg_class c WHERE c.relnamespace = 'public'::regnamespace AND c.relkind = 'r'
  UNION ALL
  SELECT 'view:' || c.relname || ':opts=' || coalesce(c.reloptions::text, '') || ':acl=' || coalesce(c.relacl::text, '')
    FROM pg_class c WHERE c.relnamespace = 'public'::regnamespace AND c.relkind = 'v'
  UNION ALL
  SELECT 'pol:' || tablename || '.' || policyname || ':' || coalesce(qual, '') || ':' || coalesce(with_check, '')
    FROM pg_policies WHERE schemaname = 'public'
  UNION ALL
  SELECT 'trg:' || tgrelid::regclass::text || '.' || tgname || ':' || tgenabled::text
    FROM pg_trigger WHERE NOT tgisinternal
  UNION ALL
  SELECT 'cmt:' || c.relname || '.' || coalesce(a.attname, '') || ':' || d.description
    FROM pg_description d
    JOIN pg_class c ON c.oid = d.objoid AND d.classoid = 'pg_class'::regclass
    LEFT JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = d.objsubid AND d.objsubid > 0
   WHERE c.relnamespace = 'public'::regnamespace
) s
ORDER BY line`

func catalog(t *testing.T, pool *pgxpool.Pool) []string {
	t.Helper()
	return queryLines(t, pool, catalogSQL)
}

func assertSameCatalog(t *testing.T, when string, want, got []string) {
	t.Helper()
	missing, extra := diffSets(want, got)
	if len(missing)+len(extra) > 0 {
		t.Fatalf("%s: the schema is not what it was\n  missing (%d): %v\n  unexpected (%d): %v", when, len(missing), missing, len(extra), extra)
	}
}

func hasEntry(snapshot []string, prefix string) bool {
	for _, line := range snapshot {
		if strings.HasPrefix(line, prefix) {
			return true
		}
	}
	return false
}

// ── unit and coverage snapshots ──────────────────────────────────────────────

// unitColumnsBefore are the columns that exist before CR-T6. They are compared
// across the migration to prove it moved nothing but context metadata.
const unitsBeforeSQL = `
SELECT knowledge_id || '|' || embedding_status || '|' || coalesce(chunk_context, '~') || '|' ||
       coalesce(embedding_model, '~') || '|' || coalesce(embedded_at::text, '~') || '|' || updated_at::text
  FROM knowledge_units ORDER BY knowledge_id`

// unitsFullSQL adds every column CR-T6 introduces.
const unitsFullSQL = `
SELECT knowledge_id || '|' || embedding_status || '|' || coalesce(chunk_context, '~') || '|' ||
       coalesce(embedding_model, '~') || '|' || coalesce(embedded_at::text, '~') || '|' || updated_at::text || '|' ||
       coalesce(context_status, '~') || '|' || coalesce(context_model, '~') || '|' || coalesce(context_provider, '~') || '|' ||
       coalesce(context_residency, '~') || '|' || coalesce(context_input_hash, '~') || '|' ||
       coalesce(embedded_context_hash, '~') || '|' || coalesce(context_generated_at::text, '~') || '|' ||
       context_attempts::text || '|' || coalesce(context_next_attempt_at::text, '~') || '|' ||
       coalesce(context_error_class, '~') || '|' || coalesce(context_lease_until::text, '~')
  FROM knowledge_units ORDER BY knowledge_id`

func statuses(t *testing.T, pool *pgxpool.Pool, orgs ...string) map[string]string {
	t.Helper()
	quoted := make([]string, len(orgs))
	for i, org := range orgs {
		quoted[i] = q(org)
	}
	out := map[string]string{}
	for _, line := range queryLines(t, pool,
		"SELECT knowledge_id || '=' || coalesce(context_status, '<NULL>') FROM knowledge_units WHERE org_id IN ("+strings.Join(quoted, ", ")+")") {
		id, status, _ := strings.Cut(line, "=")
		out[id] = status
	}
	return out
}

type coverageRow struct {
	eligible, ready, legacy, pending                 int64
	skippedZDR, skippedPolicy, skippedBudget         int64
	failedTransient, failedPermanent, statusUnknown  int64
	vectorIncoherent, readyUnverified, excludedClass int64
}

func (c coverageRow) partitionSum() int64 {
	return c.ready + c.legacy + c.pending + c.skippedZDR + c.skippedPolicy + c.skippedBudget +
		c.failedTransient + c.failedPermanent + c.statusUnknown
}

func coverage(t *testing.T, pool *pgxpool.Pool, orgs ...string) map[string]coverageRow {
	t.Helper()
	quoted := make([]string, len(orgs))
	for i, org := range orgs {
		quoted[i] = q(org)
	}
	rows, err := pool.Query(t.Context(), `SELECT org_id, eligible, ready, legacy, pending, skipped_zdr, skipped_policy,
		skipped_budget, failed_transient, failed_permanent, status_unknown, vector_incoherent, ready_unverified,
		excluded_by_classification FROM v_context_coverage WHERE org_id IN (`+strings.Join(quoted, ", ")+`)`)
	if err != nil {
		t.Fatalf("read v_context_coverage: %v", err)
	}
	defer rows.Close()
	out := map[string]coverageRow{}
	for rows.Next() {
		var org string
		var c coverageRow
		if err := rows.Scan(&org, &c.eligible, &c.ready, &c.legacy, &c.pending, &c.skippedZDR, &c.skippedPolicy,
			&c.skippedBudget, &c.failedTransient, &c.failedPermanent, &c.statusUnknown, &c.vectorIncoherent,
			&c.readyUnverified, &c.excludedClass); err != nil {
			t.Fatalf("scan coverage row: %v", err)
		}
		out[org] = c
	}
	if err := rows.Err(); err != nil {
		t.Fatal(err)
	}
	return out
}

func callBackfill(t *testing.T, pool *pgxpool.Pool, org *string) (legacy, pending int64) {
	t.Helper()
	sql := "SELECT legacy_marked, pending_marked FROM context_provenance_backfill(" + qn(org) + ")"
	if err := pool.QueryRow(t.Context(), sql).Scan(&legacy, &pending); err != nil {
		t.Fatalf("%v\nSQL: %s", err, sql)
	}
	return legacy, pending
}

// utcSecond renders a timestamptz in UTC to the second, independent of the session time zone.
func utcSecond(column string) string {
	return "to_char(" + column + " AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')"
}

func equalLines(t *testing.T, what string, want, got []string) {
	t.Helper()
	if strings.Join(want, "\n") != strings.Join(got, "\n") {
		t.Fatalf("%s changed\n--- before ---\n%s\n--- after ---\n%s", what, strings.Join(want, "\n"), strings.Join(got, "\n"))
	}
}

// ── the migration, end to end ────────────────────────────────────────────────

func TestMigrationLifecycleUpDownUp(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)

	migrate(t, db, "up", "--to", priorVersion)
	before := catalog(t, pool)
	if hasEntry(before, "tbl:org_processing_policy") || hasEntry(before, "view:v_context_coverage") ||
		hasEntry(before, "col:knowledge_units.context_status:") {
		t.Fatal("the CR-T6 objects exist before CR-T6 was applied: the test database is not the baseline it claims to be")
	}

	out := migrate(t, db, "up", "--to", cprVersion)
	if !strings.Contains(out, cprVersion) {
		t.Fatalf("migrator did not report applying %s:\n%s", cprVersion, out)
	}
	applied := catalog(t, pool)
	for _, want := range []string{
		"tbl:org_processing_policy:rls=true:",
		"view:v_context_coverage:opts={security_invoker=true}:",
		"idx:idx_ku_context_work:",
		"fn:context_text_hash(text):s:true:",
		"fn:context_provenance_backfill(text):v:false:{lock_timeout=",
		"col:knowledge_units.context_status:text:YES:",
		"col:knowledge_units.context_attempts:integer:NO:0",
		"col:knowledge_units.embedded_context_hash:text:YES:",
		"pol:org_processing_policy.org_processing_policy_rls_isolation:",
		"trg:org_processing_policy.set_org_processing_policy_updated_at:O",
	} {
		if !hasEntry(applied, want) {
			t.Errorf("after CR-T6 the catalog has no entry starting %q", want)
		}
	}
	if t.Failed() {
		t.FailNow()
	}
	if status := migrate(t, db, "status"); !regexp.MustCompile(cprVersion + `\s+context_provenance\s+applied`).MatchString(status) {
		t.Fatalf("migrator status does not show CR-T6 applied:\n%s", status)
	}

	// Down: the schema returns to exactly what it was, and the 20260824120000
	// comment that CR-T6 rewrote is restored byte for byte.
	migrate(t, db, "down", "--to", priorVersion)
	assertSameCatalog(t, "after rolling CR-T6 back", before, catalog(t, pool))
	if status := migrate(t, db, "status"); !regexp.MustCompile(cprVersion + `\s+context_provenance\s+pending`).MatchString(status) {
		t.Fatalf("migrator status does not show CR-T6 pending after the rollback:\n%s", status)
	}

	// Up again converges to the very same schema.
	migrate(t, db, "up", "--to", cprVersion)
	assertSameCatalog(t, "after applying CR-T6 a second time", applied, catalog(t, pool))

	// And the forward file is idempotent on its own: run it again on a database
	// that already has it (what a repair or a re-apply does) and nothing moves.
	execSQL(t, pool, readSQL(t, cprUp))
	assertSameCatalog(t, "after re-running the forward file", applied, catalog(t, pool))
}

func TestMigrationAddsOnlyNullableColumnsAndAFastDefault(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	got := queryLines(t, pool, `SELECT column_name || ':' || is_nullable || ':' || coalesce(column_default, '-')
		FROM information_schema.columns WHERE table_name = 'knowledge_units' AND table_schema = 'public'
		AND column_name IN ('context_status','context_model','context_provider','context_residency','context_input_hash',
		'embedded_context_hash','context_generated_at','context_attempts','context_next_attempt_at','context_error_class',
		'context_lease_until') ORDER BY column_name`)
	want := []string{
		"context_attempts:NO:0",
		"context_error_class:YES:-",
		"context_generated_at:YES:-",
		"context_input_hash:YES:-",
		"context_lease_until:YES:-",
		"context_model:YES:-",
		"context_next_attempt_at:YES:-",
		"context_provider:YES:-",
		"context_residency:YES:-",
		"context_status:YES:-",
		"embedded_context_hash:YES:-",
	}
	sort.Strings(want)
	sort.Strings(got)
	equalLines(t, "the CR-T6 column set", want, got)
}

func TestWorkQueueIndexIsPartialOverTheStatusesAWorkerPicksUp(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	def := queryString(t, pool, `SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND indexname = 'idx_ku_context_work'`)
	for _, want := range []string{"(org_id, context_next_attempt_at)", " WHERE ", "'done'", "context_status IS NULL",
		"'pending'", "'legacy'", "'failed_transient'", "'skipped_budget'"} {
		if !strings.Contains(def, want) {
			t.Errorf("idx_ku_context_work is %q: missing %q", def, want)
		}
	}
	// Rows that are settled, or re-checked per org rather than per row, are not in it.
	for _, unwanted := range []string{"'ready'", "'skipped_zdr'", "'skipped_policy'", "'failed_permanent'"} {
		if strings.Contains(def, unwanted) {
			t.Errorf("idx_ku_context_work should not cover %s: %q", unwanted, def)
		}
	}
}

// ── the legacy backfill ──────────────────────────────────────────────────────

func TestLegacyBackfillMarksLegacyAndPendingWithoutFabricatingCoherence(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)
	migrate(t, db, "up", "--to", priorVersion)

	const orgA, orgB = "org-t6-a", "org-t6-b"

	// A fixture shaped like the live corpus (206 units that carry a context, all
	// stamped updated_at after embedded_at, plus one raw unit) and then some:
	// every state the backfill has to classify, in two orgs.
	for _, d := range []struct {
		id, org, classification string
		deleted                 bool
	}{
		{"d-a-int", orgA, "internal", false},
		{"d-a-pub", orgA, "public", false},
		{"d-a-sens", orgA, "sensitive", false},
		{"d-a-mixed", orgA, "  Internal ", false}, // eligibility is LOWER(BTRIM()) like quickwit-adapter's
		{"d-a-restricted", orgA, "restricted", false},
		{"d-a-deleted", orgA, "internal", true},
		{"d-b-int", orgB, "internal", false},
	} {
		insertDocument(t, pool, d.id, d.org, d.classification, d.deleted)
	}
	legacyContext := sp("Rapport om blåbærsyltetøy fra Bergen, kapittel 3.")
	units := []unitSpec{
		{id: "u-a-legacy-1", doc: "d-a-int", org: orgA, context: legacyContext, model: sp("fixture-embed-model")},
		{id: "u-a-legacy-2", doc: "d-a-pub", org: orgA, context: legacyContext},
		{id: "u-a-legacy-3", doc: "d-a-sens", org: orgA, context: legacyContext},
		{id: "u-a-raw", doc: "d-a-int", org: orgA},
		{id: "u-a-blank", doc: "d-a-pub", org: orgA, context: sp("   ")}, // a blank context is no context
		{id: "u-a-mixed", doc: "d-a-mixed", org: orgA},
		{id: "u-a-restricted-raw", doc: "d-a-restricted", org: orgA},
		{id: "u-a-restricted-ctx", doc: "d-a-restricted", org: orgA, context: legacyContext},
		{id: "u-a-deleted", doc: "d-a-deleted", org: orgA},
		{id: "u-a-notyet", doc: "d-a-int", org: orgA, embedding: "pending"},
		{id: "u-a-failed-ctx", doc: "d-a-int", org: orgA, embedding: "failed", context: legacyContext},
		{id: "u-b-legacy", doc: "d-b-int", org: orgB, context: legacyContext},
		{id: "u-b-raw", doc: "d-b-int", org: orgB},
	}
	for _, u := range units {
		insertUnit(t, pool, u, false)
	}
	rowsBefore := queryLines(t, pool, unitsBeforeSQL)
	totalBefore := queryInt(t, pool, "SELECT count(*) FROM knowledge_units")

	migrate(t, db, "up", "--to", cprVersion)

	t.Run("the migration moves nothing but context metadata", func(t *testing.T) {
		if got := queryInt(t, pool, "SELECT count(*) FROM knowledge_units"); got != totalBefore {
			t.Fatalf("row count changed from %d to %d", totalBefore, got)
		}
		// Includes updated_at: the backfill must not make 206 units look
		// edited-after-embedding to the stale-embedding detector.
		equalLines(t, "the pre-CR-T6 columns of knowledge_units", rowsBefore, queryLines(t, pool, unitsBeforeSQL))
	})

	t.Run("every unit is classified for what is actually known", func(t *testing.T) {
		want := map[string]string{
			"u-a-legacy-1": "legacy", "u-a-legacy-2": "legacy", "u-a-legacy-3": "legacy",
			"u-a-raw": "pending", "u-a-blank": "pending", "u-a-mixed": "pending",
			"u-a-restricted-raw": "<NULL>", // never contextualized; not a pending unit
			"u-a-restricted-ctx": "legacy", // a context exists, whatever the classification
			"u-a-deleted":        "<NULL>",
			"u-a-notyet":         "<NULL>", // not embedded yet: the pipeline will classify it
			"u-a-failed-ctx":     "legacy",
			"u-b-legacy":         "legacy",
			"u-b-raw":            "pending",
		}
		got := statuses(t, pool, orgA, orgB)
		for id, status := range want {
			if got[id] != status {
				t.Errorf("%s: context_status = %s, want %s", id, got[id], status)
			}
		}
		if len(got) != len(want) {
			t.Errorf("classified %d units, fixture has %d", len(got), len(want))
		}
	})

	t.Run("no coherence, provenance or model is invented", func(t *testing.T) {
		// critic #11: a legacy vector may be raw or built from a stale context.
		// Claiming otherwise would make vector_incoherent read 0 for exactly the
		// rows it exists to expose.
		if n := queryInt(t, pool, "SELECT count(*) FROM knowledge_units WHERE context_status = 'ready'"); n != 0 {
			t.Errorf("%d units were marked ready by the backfill", n)
		}
		if n := queryInt(t, pool, `SELECT count(*) FROM knowledge_units WHERE embedded_context_hash IS NOT NULL
			OR context_model IS NOT NULL OR context_provider IS NOT NULL OR context_residency IS NOT NULL
			OR context_input_hash IS NOT NULL OR context_generated_at IS NOT NULL OR context_error_class IS NOT NULL
			OR context_lease_until IS NOT NULL OR context_attempts <> 0`); n != 0 {
			t.Errorf("%d units carry provenance the backfill cannot know", n)
		}
		// embedding_model is left exactly as it was: stamped only when a
		// verification pass re-embeds, the one moment the truth is known.
		got := queryLines(t, pool, "SELECT knowledge_id || '=' || embedding_model FROM knowledge_units WHERE embedding_model IS NOT NULL")
		if len(got) != 1 || got[0] != "u-a-legacy-1=fixture-embed-model" {
			t.Errorf("embedding_model rows after the backfill = %v, want only the fixture's own value", got)
		}
	})

	t.Run("pending units are scheduled, legacy units are not", func(t *testing.T) {
		if n := queryInt(t, pool, "SELECT count(*) FROM knowledge_units WHERE context_status = 'pending' AND context_next_attempt_at IS NULL"); n != 0 {
			t.Errorf("%d pending units have no next attempt", n)
		}
		if n := queryInt(t, pool, "SELECT count(*) FROM knowledge_units WHERE context_status IS DISTINCT FROM 'pending' AND context_next_attempt_at IS NOT NULL"); n != 0 {
			t.Errorf("%d non-pending units were scheduled", n)
		}
	})

	t.Run("coverage reports legacy separately and zero incoherent", func(t *testing.T) {
		got := coverage(t, pool, orgA, orgB)
		a := got[orgA]
		wantA := coverageRow{eligible: 6, legacy: 3, pending: 3, excludedClass: 2}
		if a != wantA {
			t.Errorf("org A coverage = %+v, want %+v", a, wantA)
		}
		b := got[orgB]
		wantB := coverageRow{eligible: 2, legacy: 1, pending: 1}
		if b != wantB {
			t.Errorf("org B coverage = %+v, want %+v", b, wantB)
		}
	})

	t.Run("the updated_at trigger is back and still works", func(t *testing.T) {
		if got := queryString(t, pool, "SELECT tgenabled::text FROM pg_trigger WHERE tgname = 'set_knowledge_units_updated_at'"); got != "O" {
			t.Fatalf("set_knowledge_units_updated_at is %q after the migration, want O (enabled)", got)
		}
		old := queryString(t, pool, "SELECT updated_at::text FROM knowledge_units WHERE knowledge_id = 'u-a-raw'")
		execSQL(t, pool, "UPDATE knowledge_units SET text = text || ' (edited)' WHERE knowledge_id = 'u-a-raw'")
		if now := queryString(t, pool, "SELECT updated_at::text FROM knowledge_units WHERE knowledge_id = 'u-a-raw'"); now == old {
			t.Error("an ordinary UPDATE no longer moves updated_at: the backfill left the trigger disabled")
		}
	})

	t.Run("running it again changes nothing", func(t *testing.T) {
		snapshot := queryLines(t, pool, unitsFullSQL)
		catalogBefore := catalog(t, pool)
		if legacy, pending := callBackfill(t, pool, nil); legacy != 0 || pending != 0 {
			t.Fatalf("a second backfill marked %d legacy and %d pending", legacy, pending)
		}
		equalLines(t, "knowledge_units after a second backfill", snapshot, queryLines(t, pool, unitsFullSQL))

		execSQL(t, pool, readSQL(t, cprUp)) // the whole forward file, again
		equalLines(t, "knowledge_units after re-running the forward file", snapshot, queryLines(t, pool, unitsFullSQL))
		assertSameCatalog(t, "after re-running the forward file", catalogBefore, catalog(t, pool))
	})

	t.Run("rows written by pre-provenance code are adopted by a re-run", func(t *testing.T) {
		// Between this migration and the CR-T8 deploy the running embedding-engine
		// keeps persisting chunk_context and leaving context_status NULL. That is
		// a supported state; the backfill is how it is closed.
		insertDocument(t, pool, "d-a-late", orgA, "internal", false)
		insertDocument(t, pool, "d-b-late", orgB, "internal", false)
		insertUnit(t, pool, unitSpec{id: "late-a-ctx", doc: "d-a-late", org: orgA, context: legacyContext}, true)
		insertUnit(t, pool, unitSpec{id: "late-a-raw", doc: "d-a-late", org: orgA}, true)
		insertUnit(t, pool, unitSpec{id: "late-b-ctx", doc: "d-b-late", org: orgB, context: legacyContext}, true)

		if got := coverage(t, pool, orgA)[orgA].statusUnknown; got != 2 {
			t.Fatalf("org A status_unknown = %d before the re-run, want 2", got)
		}
		// Scoped to one org: only that org's rows are touched.
		if legacy, pending := callBackfill(t, pool, sp(orgA)); legacy != 1 || pending != 1 {
			t.Fatalf("backfill for org A marked %d legacy and %d pending, want 1 and 1", legacy, pending)
		}
		got := statuses(t, pool, orgA, orgB)
		if got["late-a-ctx"] != "legacy" || got["late-a-raw"] != "pending" || got["late-b-ctx"] != "<NULL>" {
			t.Fatalf("after the org A backfill: %v", map[string]string{"late-a-ctx": got["late-a-ctx"], "late-a-raw": got["late-a-raw"], "late-b-ctx": got["late-b-ctx"]})
		}
		if legacy, pending := callBackfill(t, pool, nil); legacy != 1 || pending != 0 {
			t.Fatalf("unscoped backfill marked %d legacy and %d pending, want 1 and 0", legacy, pending)
		}
		if got := statuses(t, pool, orgB)["late-b-ctx"]; got != "legacy" {
			t.Fatalf("late-b-ctx = %s, want legacy", got)
		}
		if got := coverage(t, pool, orgA)[orgA].statusUnknown; got != 0 {
			t.Errorf("org A status_unknown = %d after the re-run, want 0", got)
		}
		if got := queryString(t, pool, "SELECT "+utcSecond("updated_at")+" FROM knowledge_units WHERE knowledge_id = 'late-a-ctx'"); got != "2026-09-01 10:05:00" {
			t.Errorf("the re-run moved updated_at to %s", got)
		}
	})

	t.Run("the trigger is restored to the state it was found in", func(t *testing.T) {
		const orgT = "org-t6-trg"
		insertDocument(t, pool, "d-t-int", orgT, "internal", false)
		defer execSQL(t, pool, "ALTER TABLE knowledge_units ENABLE TRIGGER set_knowledge_units_updated_at")
		for i, mode := range []struct{ state, ddl string }{
			{"O", "ENABLE TRIGGER"},
			{"A", "ENABLE ALWAYS TRIGGER"},
			{"R", "ENABLE REPLICA TRIGGER"},
			{"D", "DISABLE TRIGGER"},
		} {
			execSQL(t, pool, "ALTER TABLE knowledge_units "+mode.ddl+" set_knowledge_units_updated_at")
			id := fmt.Sprintf("trg-%d", i)
			insertUnit(t, pool, unitSpec{id: id, doc: "d-t-int", org: orgT, context: legacyContext}, true)
			if legacy, _ := callBackfill(t, pool, sp(orgT)); legacy != 1 {
				t.Fatalf("trigger state %s: backfill marked %d legacy, want 1", mode.state, legacy)
			}
			if got := queryString(t, pool, "SELECT tgenabled::text FROM pg_trigger WHERE tgname = 'set_knowledge_units_updated_at'"); got != mode.state {
				t.Fatalf("trigger state was %s before the backfill and %s after", mode.state, got)
			}
			if got := queryString(t, pool, "SELECT "+utcSecond("updated_at")+" FROM knowledge_units WHERE knowledge_id = "+q(id)); got != "2026-09-01 10:05:00" {
				t.Fatalf("trigger state %s: the backfill moved updated_at to %s", mode.state, got)
			}
		}
	})
}

// ── constraints ──────────────────────────────────────────────────────────────

func TestStatusAndProvenanceConstraints(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	const org = "org-con"
	insertDocument(t, pool, "d-con", org, "internal", false)

	var n int
	try := func(u unitSpec) error {
		n++
		u.id, u.doc, u.org = fmt.Sprintf("u-con-%d", n), "d-con", org
		_, err := pool.Exec(t.Context(), unitInsertSQL(u, true))
		return err
	}

	// Each invalid row violates exactly one constraint, so the name is unambiguous.
	for _, c := range []struct {
		name       string
		u          unitSpec
		constraint string
	}{
		{"an unknown status", unitSpec{status: sp("bogus")}, "knowledge_units_context_status_chk"},
		{"ready without a context", unitSpec{status: sp("ready")}, "knowledge_units_context_present_chk"},
		{"ready with a blank context", unitSpec{status: sp("ready"), context: sp("  ")}, "knowledge_units_context_present_chk"},
		{"legacy without a context", unitSpec{status: sp("legacy")}, "knowledge_units_context_present_chk"},
		{"failed_transient without a class", unitSpec{status: sp("failed_transient")}, "knowledge_units_context_failure_class_chk"},
		{"failed_permanent without a class", unitSpec{status: sp("failed_permanent")}, "knowledge_units_context_failure_class_chk"},
		{"skipped_zdr without a class", unitSpec{status: sp("skipped_zdr")}, "knowledge_units_context_failure_class_chk"},
		{"a free-text error class", unitSpec{status: sp("failed_transient"), class: sp("provider said: the Acme merger memo")}, "knowledge_units_context_error_class_chk"},
		{"a negative attempt count", unitSpec{attempts: -1}, "knowledge_units_context_attempts_chk"},
		{"an uppercase hash", unitSpec{hash: sp(strings.ToUpper(sha256hex("x")))}, "knowledge_units_embedded_context_hash_chk"},
		{"a short hash", unitSpec{hash: sp("abc123")}, "knowledge_units_embedded_context_hash_chk"},
		{"a malformed input hash", unitSpec{inputHash: sp("not-a-hash")}, "knowledge_units_context_input_hash_chk"},
	} {
		t.Run("rejects "+c.name, func(t *testing.T) {
			expectPgError(t, try(c.u), "23514", c.constraint)
		})
	}

	valid := []unitSpec{
		{}, // NULL status: unknown is a legal state
		{status: sp("pending")},
		{status: sp("ready"), context: sp("ctx"), hash: sp(sha256hex("ctx")), inputHash: sp(sha256hex("in"))},
		{status: sp("ready"), context: sp("ctx"), class: sp("timeout")}, // a failed regeneration; the old context stays in force
		{status: sp("legacy"), context: sp("ctx")},
		{status: sp("skipped_zdr"), class: sp("zdr_org")},
		{status: sp("skipped_zdr"), class: sp("zdr_document")},
		{status: sp("skipped_policy")},
		{status: sp("skipped_budget")},
		{status: sp("failed_transient"), class: sp("rate_limited"), attempts: 3},
		{status: sp("failed_permanent"), class: sp("too_long"), attempts: 8},
	}
	for _, class := range errorClassVocabulary {
		valid = append(valid, unitSpec{status: sp("failed_transient"), class: sp(class)})
	}
	for i, u := range valid {
		if err := try(u); err != nil {
			t.Errorf("valid row %d (%+v) was rejected: %v", i, u, err)
		}
	}

	t.Run("a purge cannot clear the context of a ready row and leave it ready", func(t *testing.T) {
		insertUnit(t, pool, unitSpec{id: "u-con-purge", doc: "d-con", org: org, context: sp("ctx"), status: sp("ready")}, true)
		_, err := pool.Exec(t.Context(), "UPDATE knowledge_units SET chunk_context = NULL WHERE knowledge_id = 'u-con-purge'")
		expectPgError(t, err, "23514", "knowledge_units_context_present_chk")
		execSQL(t, pool, "UPDATE knowledge_units SET chunk_context = NULL, context_status = 'pending' WHERE knowledge_id = 'u-con-purge'")
	})
}

func TestOrgProcessingPolicyConstraintsAndDefaults(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))

	t.Run("an absent override is the default, a new row inherits everything", func(t *testing.T) {
		execSQL(t, pool, "INSERT INTO org_processing_policy (org_id, updated_by) VALUES ('org-pol-default', 'operator')")
		got := queryString(t, pool, `SELECT contextual_retrieval || '/' || graph_extraction || '/' || budget_mode || '/' ||
			coalesce(monthly_budget_usd::text, '-') || '/' || coalesce(context_prompt_version, '-') || '/' || coalesce(reason, '-') ||
			'/' || (created_at IS NOT NULL)::text || '/' || (updated_at IS NOT NULL)::text
			FROM org_processing_policy WHERE org_id = 'org-pol-default'`)
		if got != "inherit/inherit/inherit/-/-/-/true/true" {
			t.Errorf("defaults = %s", got)
		}
	})

	for i, c := range []struct {
		name, values, constraint string
	}{
		{"an unknown contextual_retrieval", "contextual_retrieval = 'maybe'", "org_processing_policy_contextual_retrieval_chk"},
		{"an unknown graph_extraction", "graph_extraction = 'maybe'", "org_processing_policy_graph_extraction_chk"},
		{"an unknown budget_mode", "budget_mode = 'unlimited'", "org_processing_policy_budget_mode_chk"},
		{"a negative budget", "monthly_budget_usd = -1", "org_processing_policy_budget_chk"},
		{"a blank prompt version", "context_prompt_version = '  '", "org_processing_policy_prompt_version_chk"},
		{"a blank updated_by", "updated_by = '   '", "org_processing_policy_updated_by_chk"},
		{"a blank org_id", "org_id = '  '", "org_processing_policy_org_chk"},
		{"off without a reason", "contextual_retrieval = 'off'", "org_processing_policy_off_reason_chk"},
		{"off with a blank reason", "contextual_retrieval = 'off', reason = '   '", "org_processing_policy_off_reason_chk"},
		{"graph extraction off without a reason", "graph_extraction = 'off'", "org_processing_policy_off_reason_chk"},
	} {
		t.Run("rejects "+c.name, func(t *testing.T) {
			// Build the row from defaults, then override one field by name.
			override := c.values
			cols := []string{"org_id", "updated_by"}
			vals := []string{q(fmt.Sprintf("org-pol-bad-%d", i)), "'operator'"}
			for _, assignment := range strings.Split(override, ", ") {
				name, value, _ := strings.Cut(assignment, " = ")
				switch name {
				case "org_id":
					vals[0] = value
				case "updated_by":
					vals[1] = value
				default:
					cols, vals = append(cols, name), append(vals, value)
				}
			}
			_, err := pool.Exec(t.Context(), "INSERT INTO org_processing_policy ("+strings.Join(cols, ", ")+") VALUES ("+strings.Join(vals, ", ")+")")
			expectPgError(t, err, "23514", c.constraint)
		})
	}

	t.Run("accepts the legitimate overrides", func(t *testing.T) {
		for i, values := range []string{
			"contextual_retrieval = 'off', reason = 'baseline org for the A/B run'",
			"graph_extraction = 'off', reason = 'incident 4711'",
			"contextual_retrieval = 'on'",
			"monthly_budget_usd = 0",
			"budget_mode = 'alert_only'",
			"budget_mode = 'throttle', monthly_budget_usd = 25.50",
			"budget_mode = 'stop', monthly_budget_usd = 5",
			"context_prompt_version = 'ctx-v2'",
		} {
			cols, vals := []string{"org_id", "updated_by"}, []string{q(fmt.Sprintf("org-pol-ok-%d", i)), "'operator'"}
			for _, assignment := range strings.Split(values, ", ") {
				name, value, _ := strings.Cut(assignment, " = ")
				cols, vals = append(cols, name), append(vals, value)
			}
			execSQL(t, pool, "INSERT INTO org_processing_policy ("+strings.Join(cols, ", ")+") VALUES ("+strings.Join(vals, ", ")+")")
		}
	})

	t.Run("one row per org", func(t *testing.T) {
		_, err := pool.Exec(t.Context(), "INSERT INTO org_processing_policy (org_id, updated_by) VALUES ('org-pol-default', 'someone else')")
		expectPgError(t, err, "23505", "org_processing_policy_pkey")
	})

	t.Run("updated_at is maintained by the table, created_at is not", func(t *testing.T) {
		execSQL(t, pool, `INSERT INTO org_processing_policy (org_id, updated_by, created_at, updated_at)
			VALUES ('org-pol-touch', 'operator', '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z')`)
		execSQL(t, pool, "UPDATE org_processing_policy SET budget_mode = 'throttle' WHERE org_id = 'org-pol-touch'")
		got := queryString(t, pool, `SELECT (updated_at > '2020-01-01T00:00:00Z')::text || '/' || (created_at = '2020-01-01T00:00:00Z')::text
			FROM org_processing_policy WHERE org_id = 'org-pol-touch'`)
		if got != "true/true" {
			t.Errorf("updated_at moved / created_at kept = %s, want true/true", got)
		}
	})
}

// ── the canonical hash ───────────────────────────────────────────────────────

func TestContextTextHashIsTheCanonicalSHA256(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))

	// Lowercase hex of SHA-256 over the UTF-8 bytes of the stored string, no
	// normalisation: what a Rust or Go writer gets from sha256(s.as_bytes()).
	for _, in := range []string{
		"abc",
		"",
		"Rapport om blåbærsyltetøy fra Bergen ÆØÅ",
		"  padded on both sides  ",
		"two\nlines",
		"emoji 🫐 and quotes ' \" and a backslash \\",
	} {
		if got, want := queryString(t, pool, "SELECT context_text_hash("+q(in)+")"), sha256hex(in); got != want {
			t.Errorf("context_text_hash(%q) = %s, want %s", in, got, want)
		}
	}
	if got := queryString(t, pool, "SELECT context_text_hash(NULL::text)"); got != "<NULL>" {
		t.Errorf("context_text_hash(NULL) = %s: a missing context must never be hashed", got)
	}
	if queryString(t, pool, "SELECT context_text_hash('  padded  ')") == queryString(t, pool, "SELECT context_text_hash('padded')") {
		t.Error("whitespace was normalised away: the hash must cover exactly the stored bytes")
	}
}

// ── the coverage view ────────────────────────────────────────────────────────

func TestCoverageViewPartitionsEligibleUnitsAndFlagsIncoherentVectors(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	const org, other, onlyRestricted = "org-cov", "org-cov-b", "org-cov-r"

	for _, d := range []struct {
		id, org, classification string
		deleted                 bool
	}{
		{"d-cov-int", org, "internal", false},
		{"d-cov-pub", org, "public", false},
		{"d-cov-sens", org, " Sensitive ", false},
		{"d-cov-restricted", org, "restricted", false},
		{"d-cov-deleted", org, "internal", true},
		{"d-cov-b", other, "internal", false},
		{"d-cov-r", onlyRestricted, "restricted", false},
	} {
		insertDocument(t, pool, d.id, d.org, d.classification, d.deleted)
	}

	nonASCII := "Norsk kontekst: blåbærsyltetøy ÆØÅ"
	for _, u := range []unitSpec{
		// Eligible: indexed, document not deleted, classification internal/public/sensitive.
		{id: "ready-ok", doc: "d-cov-pub", status: sp("ready"), context: sp("Kontekst A"), hash: sp(sha256hex("Kontekst A")), inputHash: sp(sha256hex("in-a"))},
		{id: "ready-incoherent", doc: "d-cov-int", status: sp("ready"), context: sp("Kontekst B"), hash: sp(sha256hex("noe helt annet"))},
		{id: "ready-unverified", doc: "d-cov-int", status: sp("ready"), context: sp("Kontekst C")},
		{id: "ready-nonascii", doc: "d-cov-sens", status: sp("ready"), context: sp(nonASCII), hash: sp(sha256hex(nonASCII))},
		{id: "legacy", doc: "d-cov-int", status: sp("legacy"), context: sp("old context")},
		{id: "pending", doc: "d-cov-int", status: sp("pending")},
		{id: "skipped-zdr", doc: "d-cov-int", status: sp("skipped_zdr"), class: sp("zdr_org")},
		{id: "skipped-policy", doc: "d-cov-int", status: sp("skipped_policy")},
		{id: "skipped-budget", doc: "d-cov-int", status: sp("skipped_budget")},
		{id: "failed-transient", doc: "d-cov-int", status: sp("failed_transient"), class: sp("rate_limited")},
		{id: "failed-permanent", doc: "d-cov-int", status: sp("failed_permanent"), class: sp("too_long")},
		{id: "unknown", doc: "d-cov-sens"},
		// Not eligible: counted only as excluded, and only if indexed on a live document.
		{id: "restricted-unknown", doc: "d-cov-restricted"},
		{id: "restricted-zdr", doc: "d-cov-restricted", status: sp("skipped_zdr"), class: sp("zdr_document")},
		// Not counted at all: document deleted, or the unit is not indexed.
		{id: "deleted-doc", doc: "d-cov-deleted", status: sp("pending")},
		{id: "embedding-pending", doc: "d-cov-int", embedding: "pending", status: sp("pending")},
		{id: "embedding-failed", doc: "d-cov-int", embedding: "failed", status: sp("pending")},
	} {
		u.org = org
		insertUnit(t, pool, u, true)
	}
	insertUnit(t, pool, unitSpec{id: "b-pending", doc: "d-cov-b", org: other, status: sp("pending")}, true)
	insertUnit(t, pool, unitSpec{id: "r-only", doc: "d-cov-r", org: onlyRestricted}, true)

	got := coverage(t, pool, org, other, onlyRestricted)

	want := coverageRow{
		eligible: 12, ready: 4, legacy: 1, pending: 1,
		skippedZDR: 1, skippedPolicy: 1, skippedBudget: 1,
		failedTransient: 1, failedPermanent: 1, statusUnknown: 1,
		vectorIncoherent: 1, readyUnverified: 1, excludedClass: 2,
	}
	if got[org] != want {
		t.Errorf("org %s coverage\n  got  %+v\n  want %+v", org, got[org], want)
	}
	if want := (coverageRow{eligible: 1, pending: 1}); got[other] != want {
		t.Errorf("org %s coverage = %+v, want %+v", other, got[other], want)
	}
	// An org with only restricted material still has a row: visible, never silent.
	if want := (coverageRow{excludedClass: 1}); got[onlyRestricted] != want {
		t.Errorf("org %s coverage = %+v, want %+v", onlyRestricted, got[onlyRestricted], want)
	}
	// The identity a coverage gate relies on: eligible is exactly the sum of the statuses.
	for orgID, row := range got {
		if row.eligible != row.partitionSum() {
			t.Errorf("org %s: eligible %d != sum of the statuses %d", orgID, row.eligible, row.partitionSum())
		}
	}
	// Incoherent means a recorded hash that is not the hash of the stored context.
	// A NULL hash is "unverified" (counted separately), never "incoherent", and a
	// non-ASCII context hashes the same in the database and in Go.
	if got[org].vectorIncoherent != 1 || got[org].readyUnverified != 1 {
		t.Errorf("vector_incoherent = %d, ready_unverified = %d, want 1 and 1", got[org].vectorIncoherent, got[org].readyUnverified)
	}
}

// ── row-level security, as the runtime role, through the real helper ─────────

func scopedStrings(t *testing.T, pool *pgxpool.Pool, org, sql string) []string {
	t.Helper()
	out, err := orgscope.InOrgScope(t.Context(), pool, org,
		func(ctx context.Context, tx pgx.Tx) ([]string, error) {
			rows, err := tx.Query(ctx, sql)
			if err != nil {
				return nil, err
			}
			defer rows.Close()
			var res []string
			for rows.Next() {
				var s string
				if err := rows.Scan(&s); err != nil {
					return nil, err
				}
				res = append(res, s)
			}
			return res, rows.Err()
		})
	if err != nil {
		t.Fatalf("scoped read as %s: %v\nSQL: %s", org, err, sql)
	}
	return out
}

func scopedExec(t *testing.T, pool *pgxpool.Pool, org, sql string) (int64, error) {
	t.Helper()
	var affected int64
	err := orgscope.WithOrgScope(t.Context(), pool, org, func(tx pgx.Tx) error {
		tag, err := tx.Exec(t.Context(), sql)
		if err != nil {
			return err
		}
		affected = tag.RowsAffected()
		return nil
	})
	return affected, err
}

func TestRuntimeRoleReallyIsSubjectToRowLevelSecurity(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	// If dataplane_app could bypass RLS every isolation test below would pass
	// vacuously. Assert the premise.
	if got := queryString(t, pool, "SELECT rolsuper::text || '/' || rolbypassrls::text || '/' || rolcanlogin::text FROM pg_roles WHERE rolname = 'dataplane_app'"); got != "false/false/false" {
		t.Fatalf("dataplane_app (super/bypassrls/login) = %s, want false/false/false", got)
	}
	if got := scopedStrings(t, pool, "org-any", "SELECT current_user::text"); len(got) != 1 || got[0] != "dataplane_app" {
		t.Fatalf("a scoped transaction runs as %v, want dataplane_app", got)
	}
}

func TestOrgProcessingPolicyIsIsolatedPerOrgForTheRuntimeRole(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	execSQL(t, pool, `INSERT INTO org_processing_policy (org_id, contextual_retrieval, reason, updated_by) VALUES
		('org-rls-a', 'off', 'incident 1', 'operator-a'),
		('org-rls-b', 'inherit', NULL, 'operator-b')`)

	const listSQL = "SELECT org_id FROM org_processing_policy WHERE org_id LIKE 'org-rls-%' ORDER BY org_id"
	if got := scopedStrings(t, pool, "org-rls-a", listSQL); strings.Join(got, ",") != "org-rls-a" {
		t.Errorf("org A sees %v", got)
	}
	if got := scopedStrings(t, pool, "org-rls-b", listSQL); strings.Join(got, ",") != "org-rls-b" {
		t.Errorf("org B sees %v", got)
	}
	if got := scopedStrings(t, pool, "org-rls-c", listSQL); len(got) != 0 {
		t.Errorf("an org with no policy sees %v", got)
	}

	t.Run("org B cannot change or delete org A's row", func(t *testing.T) {
		if n, err := scopedExec(t, pool, "org-rls-b", "UPDATE org_processing_policy SET contextual_retrieval = 'on', reason = NULL WHERE org_id = 'org-rls-a'"); err != nil || n != 0 {
			t.Errorf("cross-org UPDATE affected %d rows, err %v; want 0 rows", n, err)
		}
		if n, err := scopedExec(t, pool, "org-rls-b", "DELETE FROM org_processing_policy WHERE org_id = 'org-rls-a'"); err != nil || n != 0 {
			t.Errorf("cross-org DELETE affected %d rows, err %v; want 0 rows", n, err)
		}
		if got := queryString(t, pool, "SELECT contextual_retrieval || '/' || reason FROM org_processing_policy WHERE org_id = 'org-rls-a'"); got != "off/incident 1" {
			t.Errorf("org A's row is now %s", got)
		}
	})

	t.Run("org B cannot write a row for another org or move its own", func(t *testing.T) {
		_, err := scopedExec(t, pool, "org-rls-b", "INSERT INTO org_processing_policy (org_id, updated_by) VALUES ('org-rls-forged', 'x')")
		expectPgError(t, err, "42501", "")
		_, err = scopedExec(t, pool, "org-rls-b", "UPDATE org_processing_policy SET org_id = 'org-rls-moved' WHERE org_id = 'org-rls-b'")
		expectPgError(t, err, "42501", "")
		if n := queryInt(t, pool, "SELECT count(*) FROM org_processing_policy WHERE org_id IN ('org-rls-forged', 'org-rls-moved')"); n != 0 {
			t.Errorf("%d rows were written across the org boundary", n)
		}
	})

	t.Run("an org can write its own row", func(t *testing.T) {
		n, err := scopedExec(t, pool, "org-rls-own", "INSERT INTO org_processing_policy (org_id, updated_by) VALUES ('org-rls-own', 'own operator')")
		if err != nil || n != 1 {
			t.Fatalf("own-org INSERT: %d rows, err %v", n, err)
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
		if err := tx.QueryRow(t.Context(), "SELECT count(*) FROM org_processing_policy").Scan(&n); err != nil {
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
		got := queryString(t, pool, fmt.Sprintf(policySQL, "public.org_processing_policy"))
		if got != want {
			t.Errorf("org_processing_policy policy = %s\n   knowledge_units policy = %s", got, want)
		}
		if got := queryString(t, pool, "SELECT relrowsecurity::text FROM pg_class WHERE oid = 'public.org_processing_policy'::regclass"); got != "true" {
			t.Errorf("row-level security is not enabled on org_processing_policy: %s", got)
		}
	})
}

func TestCoverageViewHonoursRowLevelSecurity(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	insertDocument(t, pool, "d-rls-a", "org-rls-view-a", "internal", false)
	insertDocument(t, pool, "d-rls-b", "org-rls-view-b", "internal", false)
	insertUnit(t, pool, unitSpec{id: "rls-a-1", doc: "d-rls-a", org: "org-rls-view-a", status: sp("pending")}, true)
	insertUnit(t, pool, unitSpec{id: "rls-a-2", doc: "d-rls-a", org: "org-rls-view-a", status: sp("pending")}, true)
	insertUnit(t, pool, unitSpec{id: "rls-b-1", doc: "d-rls-b", org: "org-rls-view-b", status: sp("pending")}, true)

	const viewSQL = "SELECT org_id || ':' || eligible::text FROM v_context_coverage WHERE org_id LIKE 'org-rls-view-%' ORDER BY org_id"

	// The operator connection (a superuser) sees every org, as it must.
	if got := queryLines(t, pool, viewSQL); strings.Join(got, ",") != "org-rls-view-a:2,org-rls-view-b:1" {
		t.Fatalf("the unscoped connection sees %v", got)
	}
	// The runtime role sees only its own org, through the view.
	if got := scopedStrings(t, pool, "org-rls-view-a", viewSQL); strings.Join(got, ",") != "org-rls-view-a:2" {
		t.Errorf("org A sees %v through v_context_coverage", got)
	}
	if got := scopedStrings(t, pool, "org-rls-view-b", viewSQL); strings.Join(got, ",") != "org-rls-view-b:1" {
		t.Errorf("org B sees %v through v_context_coverage", got)
	}
	if got := scopedStrings(t, pool, "org-rls-view-c", viewSQL); len(got) != 0 {
		t.Errorf("an org with no units sees %v through v_context_coverage", got)
	}

	t.Run("it is security_invoker, and without that the same query would leak", func(t *testing.T) {
		if got := queryString(t, pool, "SELECT coalesce(reloptions::text, '') FROM pg_class WHERE oid = 'public.v_context_coverage'::regclass"); got != "{security_invoker=true}" {
			t.Fatalf("v_context_coverage reloptions = %q", got)
		}
		// Negative control: the identical SELECT, as a view that runs with its
		// owner's rights. If this does not leak, the assertions above prove
		// nothing about security_invoker.
		def := queryString(t, pool, "SELECT pg_get_viewdef('public.v_context_coverage'::regclass, true)")
		execSQL(t, pool, "CREATE VIEW v_context_coverage_unsafe AS "+def)
		defer execSQL(t, pool, "DROP VIEW IF EXISTS v_context_coverage_unsafe")
		execSQL(t, pool, "GRANT SELECT ON v_context_coverage_unsafe TO dataplane_app")
		leaked := scopedStrings(t, pool, "org-rls-view-a", "SELECT org_id FROM v_context_coverage_unsafe WHERE org_id LIKE 'org-rls-view-%' ORDER BY org_id")
		if strings.Join(leaked, ",") != "org-rls-view-a,org-rls-view-b" {
			t.Fatalf("the negative control did not leak (%v): this test cannot detect a missing security_invoker", leaked)
		}
	})
}

func TestRuntimeRolePrivilegesAreLeastPrivilege(t *testing.T) {
	pool := openPool(t, migratedDatabase(t))
	priv := func(relation, privilege string) string {
		return queryString(t, pool, fmt.Sprintf("SELECT has_table_privilege('dataplane_app', 'public.%s', '%s')::text", relation, privilege))
	}
	for _, p := range []string{"SELECT", "INSERT", "UPDATE", "DELETE"} {
		if got := priv("org_processing_policy", p); got != "true" {
			t.Errorf("dataplane_app lacks %s on org_processing_policy", p)
		}
	}
	if got := priv("v_context_coverage", "SELECT"); got != "true" {
		t.Error("dataplane_app cannot read v_context_coverage")
	}
	for _, p := range []string{"INSERT", "UPDATE", "DELETE"} {
		if got := priv("v_context_coverage", p); got != "false" {
			t.Errorf("dataplane_app has %s on the read-only view v_context_coverage", p)
		}
	}

	// The backfill suspends a trigger, which needs table ownership: operators
	// only. The runtime role must not be able to call it.
	//
	// Assert the PRIVILEGE itself, not only the failure. A call made as dataplane_app
	// is refused with 42501 either way: without EXECUTE, or (if EXECUTE were left on
	// PUBLIC) because the function body cannot ALTER TABLE knowledge_units as a
	// non-owner. Checking only the error let a missing REVOKE ALL ... FROM PUBLIC
	// survive (reviewer mutation M8).
	if got := queryString(t, pool, "SELECT has_function_privilege('dataplane_app', 'public.context_provenance_backfill(text)', 'EXECUTE')::text"); got != "false" {
		t.Errorf("dataplane_app can EXECUTE context_provenance_backfill (not revoked from PUBLIC): %s", got)
	}
	_, err := scopedExec(t, pool, "org-priv", "SELECT * FROM context_provenance_backfill()")
	expectPgError(t, err, "42501", "")
}

// ── the migration's own post-condition ───────────────────────────────────────

// The DO $post$ block is what stops a wrong backfill from shipping: it runs inside
// the migrator's transaction, so raising aborts the whole migration. It is only
// worth anything if it really raises, so run the block itself against a database
// that violates each half of it, and against ones that do not.
func TestPostConditionAbortsOnAnUnclassifiedUnit(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)
	migrate(t, db, "up", "--to", cprVersion)

	block := statementBody(t, readSQL(t, cprUp), `DO\s+\$post\$`, `\$post\$;`)
	execSQL(t, pool, block) // nothing to classify: it passes

	const org = "org-post"
	insertDocument(t, pool, "d-post", org, "internal", false)
	insertDocument(t, pool, "d-post-restricted", org, "restricted", false)
	insertDocument(t, pool, "d-post-deleted", org, "internal", true)

	// Units the backfill is NOT responsible for classifying: not eligible, or
	// already classified. None of them may trip the block.
	for _, u := range []unitSpec{
		{id: "post-ok-restricted", doc: "d-post-restricted", org: org},
		{id: "post-ok-deleted", doc: "d-post-deleted", org: org},
		{id: "post-ok-notyet", doc: "d-post", org: org, embedding: "pending"},
		{id: "post-ok-classified", doc: "d-post", org: org, status: sp("pending")},
	} {
		insertUnit(t, pool, u, true)
	}
	execSQL(t, pool, block)

	for _, c := range []struct {
		name string
		u    unitSpec
		want string
	}{
		{"a unit that carries a context and has no status",
			unitSpec{id: "post-bad-ctx", doc: "d-post-restricted", org: org, context: sp("a stored context")},
			"with a context and no status"},
		{"an eligible unit without a status",
			unitSpec{id: "post-bad-raw", doc: "d-post", org: org},
			"eligible unit(s) without a status"},
	} {
		t.Run(c.name, func(t *testing.T) {
			insertUnit(t, pool, c.u, true)
			_, err := pool.Exec(t.Context(), block)
			expectPgError(t, err, "P0001", "")
			if !strings.Contains(err.Error(), c.want) {
				t.Errorf("post-condition error = %v, want it to mention %q", err, c.want)
			}
			execSQL(t, pool, "DELETE FROM knowledge_units WHERE knowledge_id = "+q(c.u.id))
			execSQL(t, pool, block) // and it passes again once the unit is gone
		})
	}
}
