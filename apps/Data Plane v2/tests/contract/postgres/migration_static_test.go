package postgrescontract

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// These tests read the migration files and need no database, so they run
// everywhere, always: in a bare `go test`, in CI and on a laptop. They pin the
// decisions that the later contextual-retrieval tasks (CR-T7, CR-T8, CR-T9a,
// CR-T11) are written against. The database tests in this package prove the
// migration BEHAVES; these prove it still says what those tasks rely on.

func TestContextProvenanceMigrationIsAPairWithAFreeVersion(t *testing.T) {
	entries, err := os.ReadDir(migrationsDir(t))
	if err != nil {
		t.Fatalf("read migrations dir: %v", err)
	}
	// The migrator's own filename pattern and skip rules (tools/migrator/main.go
	// loadMigrations): .disabled and .down.sql files are not forward migrations.
	forward := regexp.MustCompile(`^(\d{14})_(.+)\.sql$`)
	byVersion := map[string][]string{}
	for _, e := range entries {
		name := e.Name()
		if e.IsDir() || strings.HasSuffix(name, ".disabled") || strings.HasSuffix(name, ".down.sql") {
			continue
		}
		if m := forward.FindStringSubmatch(name); m != nil {
			byVersion[m[1]] = append(byVersion[m[1]], name)
		}
	}
	for version, names := range byVersion {
		if len(names) > 1 {
			t.Errorf("version %s is claimed by %d forward migrations %v: the migrator would apply one and record the other as applied",
				version, len(names), names)
		}
	}
	if got := byVersion[cprVersion]; len(got) != 1 || got[0] != cprUp {
		t.Errorf("forward migration for %s = %v, want exactly [%s]", cprVersion, got, cprUp)
	}
	if cprVersion <= priorVersion {
		t.Errorf("%s must sort after the previously applied %s or the migrator never applies it on a live database", cprVersion, priorVersion)
	}
	if _, err := os.Stat(filepath.Join(migrationsDir(t), cprDown)); err != nil {
		t.Errorf("the migrator refuses to roll back a migration without a paired .down.sql: %v", err)
	}
}

func TestContextProvenanceUpIsIdempotentByConstruction(t *testing.T) {
	sql := code(readSQL(t, cprUp))

	// The migrator wraps each file in a transaction: a file must not manage one.
	if tx := regexp.MustCompile(`(?im)^\s*(BEGIN|COMMIT|ROLLBACK|START\s+TRANSACTION)\s*;`).FindString(sql); tx != "" {
		t.Errorf("migration contains transaction control %q; the migrator owns the transaction", strings.TrimSpace(tx))
	}
	if strings.Contains(strings.ToUpper(sql), "CONCURRENTLY") {
		t.Error("CREATE INDEX CONCURRENTLY cannot run inside the migrator's transaction")
	}

	for _, m := range regexp.MustCompile(`(?i)\bADD\s+COLUMN\s+(IF\s+NOT\s+EXISTS\s+)?(\w+)`).FindAllStringSubmatch(sql, -1) {
		if m[1] == "" {
			t.Errorf("ADD COLUMN %s lacks IF NOT EXISTS", m[2])
		}
	}
	for _, m := range regexp.MustCompile(`(?i)\bCREATE\s+TABLE\s+(IF\s+NOT\s+EXISTS\s+)?(\w+)`).FindAllStringSubmatch(sql, -1) {
		if m[1] == "" {
			t.Errorf("CREATE TABLE %s lacks IF NOT EXISTS", m[2])
		}
	}
	for _, m := range regexp.MustCompile(`(?i)\bCREATE\s+(?:UNIQUE\s+)?INDEX\s+(IF\s+NOT\s+EXISTS\s+)?(\w+)`).FindAllStringSubmatch(sql, -1) {
		if m[1] == "" {
			t.Errorf("CREATE INDEX %s lacks IF NOT EXISTS", m[2])
		}
	}
	for _, m := range regexp.MustCompile(`(?i)\bCREATE\s+(OR\s+REPLACE\s+)?(FUNCTION|VIEW|TRIGGER)\s+(\w+)`).FindAllStringSubmatch(sql, -1) {
		if m[1] == "" {
			t.Errorf("CREATE %s %s lacks OR REPLACE", strings.ToUpper(m[2]), m[3])
		}
	}
	for _, name := range matches(regexp.MustCompile(`(?i)\bADD\s+CONSTRAINT\s+(\w+)`), sql) {
		drop := regexp.MustCompile(`(?i)DROP\s+CONSTRAINT\s+IF\s+EXISTS\s+` + regexp.QuoteMeta(name) + `\b`)
		if !drop.MatchString(sql) {
			t.Errorf("ADD CONSTRAINT %s is not preceded by DROP CONSTRAINT IF EXISTS %s, so a second run fails with duplicate_object", name, name)
		}
	}
	for _, name := range matches(regexp.MustCompile(`(?i)\bCREATE\s+POLICY\s+(\w+)`), sql) {
		drop := regexp.MustCompile(`(?i)DROP\s+POLICY\s+IF\s+EXISTS\s+` + regexp.QuoteMeta(name) + `\b`)
		if !drop.MatchString(sql) {
			t.Errorf("CREATE POLICY %s is not preceded by DROP POLICY IF EXISTS %s", name, name)
		}
	}
}

func TestContextProvenanceDownReversesEverythingUpCreates(t *testing.T) {
	up := code(readSQL(t, cprUp))
	down := code(readSQL(t, cprDown))

	// Each rule: a thing the forward file creates, and the statement the rollback
	// must contain for it. Constraints and the table's own index/policy/trigger go
	// with their parent, but the named ones are asserted so a rename is caught.
	rules := []struct {
		what    string
		created *regexp.Regexp
		drop    func(name string) *regexp.Regexp
	}{
		{"column", regexp.MustCompile(`(?i)\bADD\s+COLUMN\s+IF\s+NOT\s+EXISTS\s+(\w+)`),
			func(n string) *regexp.Regexp {
				return regexp.MustCompile(`(?i)DROP\s+COLUMN\s+IF\s+EXISTS\s+` + n + `\b`)
			}},
		{"constraint", regexp.MustCompile(`(?i)\bADD\s+CONSTRAINT\s+(\w+)`),
			func(n string) *regexp.Regexp {
				return regexp.MustCompile(`(?i)DROP\s+CONSTRAINT\s+IF\s+EXISTS\s+` + n + `\b`)
			}},
		{"index", regexp.MustCompile(`(?i)\bCREATE\s+INDEX\s+IF\s+NOT\s+EXISTS\s+(\w+)`),
			func(n string) *regexp.Regexp {
				return regexp.MustCompile(`(?i)DROP\s+INDEX\s+IF\s+EXISTS\s+` + n + `\b`)
			}},
		{"function", regexp.MustCompile(`(?i)\bCREATE\s+OR\s+REPLACE\s+FUNCTION\s+(\w+)\s*\(`),
			func(n string) *regexp.Regexp {
				return regexp.MustCompile(`(?i)DROP\s+FUNCTION\s+IF\s+EXISTS\s+` + n + `\s*\(`)
			}},
		{"table", regexp.MustCompile(`(?i)\bCREATE\s+TABLE\s+IF\s+NOT\s+EXISTS\s+(\w+)`),
			func(n string) *regexp.Regexp {
				return regexp.MustCompile(`(?i)DROP\s+TABLE\s+IF\s+EXISTS\s+(public\.)?` + n + `\b`)
			}},
		{"view", regexp.MustCompile(`(?i)\bCREATE\s+OR\s+REPLACE\s+VIEW\s+(\w+)`),
			func(n string) *regexp.Regexp {
				return regexp.MustCompile(`(?i)DROP\s+VIEW\s+IF\s+EXISTS\s+` + n + `\b`)
			}},
	}
	for _, rule := range rules {
		names := matches(rule.created, up)
		if len(names) == 0 {
			t.Errorf("found no %s in the forward migration: the extraction pattern is stale", rule.what)
		}
		for _, name := range names {
			if !rule.drop(regexp.QuoteMeta(name)).MatchString(down) {
				t.Errorf("rollback does not drop %s %s", rule.what, name)
			}
		}
	}
}

func TestContextProvenanceVocabulariesArePinned(t *testing.T) {
	raw := readSQL(t, cprUp)
	sql := code(raw)

	statuses := quotedTokens(constraintCheckBody(t, sql, "knowledge_units_context_status_chk"))
	if missing, extra := diffSets(statusVocabulary, statuses); len(missing)+len(extra) > 0 {
		t.Errorf("context_status CHECK differs from the pinned vocabulary: missing %v, unexpected %v", missing, extra)
	}
	classes := quotedTokens(constraintCheckBody(t, sql, "knowledge_units_context_error_class_chk"))
	if missing, extra := diffSets(errorClassVocabulary, classes); len(missing)+len(extra) > 0 {
		t.Errorf("context_error_class CHECK differs from the pinned vocabulary: missing %v, unexpected %v "+
			"(CR-T7 maps outcomes onto these labels; a label the CHECK rejects turns a typed outcome into a failed write)",
			missing, extra)
	}

	// Statuses that must explain themselves with a class: the failures, and a ZDR
	// skip (which has to say whether the document or the org caused it).
	explained := quotedTokens(constraintCheckBody(t, sql, "knowledge_units_context_failure_class_chk"))
	if !sameSet(explained, []string{"failed_transient", "failed_permanent", "skipped_zdr"}) {
		t.Errorf("failure-class CHECK covers %v, want failed_transient, failed_permanent, skipped_zdr", explained)
	}
	// Statuses that assert a context exists.
	present := quotedTokens(constraintCheckBody(t, sql, "knowledge_units_context_present_chk"))
	if !sameSet(present, []string{"ready", "legacy"}) {
		t.Errorf("context-present CHECK covers %v, want ready and legacy", present)
	}

	// The coverage view has one column per status and the partition remainder, so
	// `eligible` is always the sum of the others.
	view := statementBody(t, sql, `CREATE OR REPLACE VIEW v_context_coverage`, `GROUP BY u\.org_id;`)
	for _, status := range statusVocabulary {
		if !regexp.MustCompile(`\bAS\s+` + status + `\b`).MatchString(view) {
			t.Errorf("v_context_coverage has no column for status %q", status)
		}
	}
	for _, column := range []string{"eligible", "status_unknown", "vector_incoherent", "ready_unverified", "excluded_by_classification"} {
		if !regexp.MustCompile(`\bAS\s+` + column + `\b`).MatchString(view) {
			t.Errorf("v_context_coverage has no %q column", column)
		}
	}

	// The work-queue index only names real statuses, and includes the ones a
	// worker has to pick up again.
	index := statementBody(t, sql, `CREATE INDEX IF NOT EXISTS idx_ku_context_work`, `;`)
	for _, token := range quotedTokens(index) {
		if token == "done" { // embedding_status, not a context status
			continue
		}
		if !contains(statusVocabulary, token) {
			t.Errorf("idx_ku_context_work names %q, which is not a context status", token)
		}
	}
	for _, status := range []string{"pending", "legacy", "failed_transient", "skipped_budget"} {
		if !strings.Contains(index, "'"+status+"'") {
			t.Errorf("idx_ku_context_work does not cover %q, so the reconciler would scan the whole table for it", status)
		}
	}

	// The column comment is what an operator reads in psql.
	comment := statementBody(t, raw, `COMMENT ON COLUMN knowledge_units\.context_status IS`, `;`)
	for _, status := range statusVocabulary {
		if !strings.Contains(comment, status) {
			t.Errorf("COMMENT ON COLUMN context_status does not list %q", status)
		}
	}
}

func contains(set []string, s string) bool {
	for _, x := range set {
		if x == s {
			return true
		}
	}
	return false
}

func TestContextTextHashVolatilityIsDeclaredHonestly(t *testing.T) {
	sql := code(readSQL(t, cprUp))
	fn := statementBody(t, sql, `CREATE OR REPLACE FUNCTION context_text_hash`, `\$fn\$;`)

	// convert_to() is STABLE in Postgres (it consults the conversion catalog), so a
	// wrapper that calls it must not be declared IMMUTABLE: that is a promise the
	// body cannot keep, and it would let someone index or generate a column from it.
	if regexp.MustCompile(`(?i)\bIMMUTABLE\b`).MatchString(fn) {
		t.Error("context_text_hash is declared IMMUTABLE but calls the STABLE convert_to()")
	}
	if !regexp.MustCompile(`(?i)\bSTABLE\b`).MatchString(fn) {
		t.Error("context_text_hash must be declared STABLE")
	}
	if !regexp.MustCompile(`(?i)\bSTRICT\b`).MatchString(fn) {
		t.Error("context_text_hash must be STRICT: a missing context is never hashed")
	}
	// The canonical bytes: UTF-8 of the stored string, lowercase hex of the digest.
	for _, want := range []string{"sha256(", "convert_to(p_text, 'UTF8')", "'hex'"} {
		if !strings.Contains(fn, want) {
			t.Errorf("context_text_hash no longer contains %q: the canonical definition changed", want)
		}
	}
}

func TestCoverageViewAndPolicyAreRowLevelSecurityAware(t *testing.T) {
	sql := code(readSQL(t, cprUp))

	// Without security_invoker the view runs as its owner (a superuser, which
	// bypasses RLS) and a scoped dataplane_app read would return every org.
	if !regexp.MustCompile(`(?is)CREATE\s+OR\s+REPLACE\s+VIEW\s+v_context_coverage\s+WITH\s*\(\s*security_invoker\s*=\s*true\s*\)`).MatchString(sql) {
		t.Error("v_context_coverage must be created WITH (security_invoker = true)")
	}
	// Default privileges grant INSERT/UPDATE/DELETE on every new relation to the
	// runtime role; a read-only view must not keep them.
	if !regexp.MustCompile(`(?is)GRANT\s+SELECT\s+ON\s+(public\.)?v_context_coverage\s+TO\s+dataplane_app`).MatchString(sql) {
		t.Error("the runtime role is not granted SELECT on v_context_coverage")
	}
	if !regexp.MustCompile(`(?is)REVOKE\s+INSERT\s*,\s*UPDATE\s*,\s*DELETE\s+ON\s+(public\.)?v_context_coverage\s+FROM\s+dataplane_app`).MatchString(sql) {
		t.Error("write privileges on v_context_coverage are not revoked from the runtime role")
	}

	// A new org table is not protected automatically (20260809120000 says so).
	if !regexp.MustCompile(`(?is)ALTER\s+TABLE\s+(public\.)?org_processing_policy\s+ENABLE\s+ROW\s+LEVEL\s+SECURITY`).MatchString(sql) {
		t.Error("org_processing_policy does not ENABLE ROW LEVEL SECURITY")
	}
	policy := statementBody(t, sql, `CREATE POLICY org_processing_policy_rls_isolation`, `;`)
	for _, clause := range []string{"USING", "WITH CHECK"} {
		if !strings.Contains(policy, clause+" (org_id = current_setting('app.current_org', true))") {
			t.Errorf("org_processing_policy policy is missing %s (org_id = current_setting('app.current_org', true))", clause)
		}
	}
	if !regexp.MustCompile(`(?is)GRANT\s+SELECT\s*,\s*INSERT\s*,\s*UPDATE\s*,\s*DELETE\s+ON\s+(public\.)?org_processing_policy\s+TO\s+dataplane_app`).MatchString(sql) {
		t.Error("the runtime role is not granted access to org_processing_policy")
	}
}

func TestMigrationBoundsItsLockWaitBeforeTouchingTheTable(t *testing.T) {
	sql := code(readSQL(t, cprUp))
	// 20260824120000 documents builder transactions that hold FOR UPDATE locks on
	// knowledge_units for minutes. ALTER TABLE queues behind them and, while it
	// queues, blocks every other reader and writer of the table. A bounded wait
	// turns that into a fast, harmless, retryable failure.
	lock := regexp.MustCompile(`(?i)SET\s+LOCAL\s+lock_timeout\s*=`).FindStringIndex(sql)
	alter := regexp.MustCompile(`(?i)\bALTER\s+TABLE\b`).FindStringIndex(sql)
	if lock == nil {
		t.Fatal("migration does not set a lock_timeout")
	}
	if alter == nil || lock[0] > alter[0] {
		t.Error("lock_timeout must be set before the first ALTER TABLE")
	}
	// The re-runnable backfill takes a table lock of its own (to suspend the
	// updated_at trigger), so it carries its own bound.
	fn := statementBody(t, sql, `CREATE OR REPLACE FUNCTION context_provenance_backfill`, `\$fn\$;`)
	if !regexp.MustCompile(`(?i)\bSET\s+lock_timeout\s*=`).MatchString(fn) {
		t.Error("context_provenance_backfill must bound its own lock wait (function-level SET lock_timeout)")
	}
	// The rollback drops columns, which needs the same exclusive lock.
	down := code(readSQL(t, cprDown))
	if !regexp.MustCompile(`(?i)SET\s+LOCAL\s+lock_timeout\s*=`).MatchString(down) {
		t.Error("the rollback does not bound its lock wait")
	}
}

func TestLegacyBackfillNeverFabricatesProvenance(t *testing.T) {
	sql := code(readSQL(t, cprUp))

	// Owner ruling and critic #11: a legacy row's vector may be raw or built from a
	// stale context, and nothing here can know. So the backfill records exactly two
	// facts (a context exists: legacy; no context but eligible: pending) and
	// invents none: no coherence hash, no model, no provenance, no `ready`.
	body := statementBody(t, sql, `CREATE OR REPLACE FUNCTION context_provenance_backfill`, `\$fn\$;`)
	for _, forbidden := range []string{
		"embedded_context_hash", "embedding_model", "context_model", "context_provider",
		"context_residency", "context_input_hash", "context_generated_at", "context_error_class",
	} {
		if strings.Contains(body, forbidden) {
			t.Errorf("the backfill touches %s; it may only set context_status and context_next_attempt_at", forbidden)
		}
	}
	if strings.Contains(body, "'ready'") {
		t.Error("the backfill marks rows ready; legacy rows must stay legacy until a verification pass proves the vector")
	}
	// updated_at is the stale-embedding detector's input; a backfill of metadata
	// must not move it.
	if regexp.MustCompile(`(?i)\bupdated_at\s*=`).MatchString(body) {
		t.Error("the backfill writes updated_at")
	}
	for _, want := range []string{"context_status = 'legacy'", "context_status = 'pending'"} {
		if !strings.Contains(body, want) {
			t.Errorf("the backfill no longer contains %q", want)
		}
	}

	// And nowhere in the file is a model or a hash assigned.
	for _, column := range []string{"embedding_model", "embedded_context_hash"} {
		if regexp.MustCompile(`(?i)\b` + column + `\s*=`).MatchString(sql) {
			t.Errorf("the migration assigns %s; that is only knowable when a vector is (re)built", column)
		}
	}
}

func TestMigrationVerifiesItsOwnBackfill(t *testing.T) {
	sql := code(readSQL(t, cprUp))
	// Same pattern as 20260620130000_add_document_ownership: a data migration
	// proves its post-condition inside the transaction, so a wrong result aborts
	// the deploy instead of shipping.
	if !regexp.MustCompile(`DO\s+\$post\$`).MatchString(sql) {
		t.Fatal("migration has no post-condition block (DO $post$ ... $post$)")
	}
	block := statementBody(t, sql, `DO\s+\$post\$`, `\$post\$;`)
	// Two halves (a unit with a context, an eligible unit) and each must raise.
	if strings.Count(block, "RAISE EXCEPTION") < 2 {
		t.Error("the post-condition block must raise for both a unit with a context and an eligible unit that have no status")
	}
	// It must run after the backfill, not before it.
	call := regexp.MustCompile(`(?i)SELECT\s+\*\s+FROM\s+context_provenance_backfill\(\)`).FindStringIndex(sql)
	post := regexp.MustCompile(`DO\s+\$post\$`).FindStringIndex(sql)
	if call == nil || post == nil || post[0] < call[0] {
		t.Error("the post-condition must come after the backfill call")
	}
}
