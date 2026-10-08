package postgrescontract

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// The graph extraction state migration (design task CR-T12). Like the other *_static_test.go
// files these read files and need no database, so they run everywhere, always. They pin the
// vocabulary graph-index writes, the rules its SQL relies on and the shape of the rollback.

const (
	gesVersion = "20261003150000"
	gesUp      = "20261003150000_graph_extraction_state.sql"
	gesDown    = "20261003150000_graph_extraction_state.down.sql"
	gesState   = "services/graph-index-rs/src/state.rs"
)

func TestGraphExtractionStateMigrationIsAPairAfterCostEventsWithAFreeVersion(t *testing.T) {
	entries, err := os.ReadDir(migrationsDir(t))
	if err != nil {
		t.Fatalf("read migrations dir: %v", err)
	}
	forward := regexp.MustCompile(`^(\d{14})_(.+)\.sql$`)
	var claimed []string
	for _, e := range entries {
		name := e.Name()
		if e.IsDir() || strings.HasSuffix(name, ".disabled") || strings.HasSuffix(name, ".down.sql") {
			continue
		}
		if m := forward.FindStringSubmatch(name); m != nil && m[1] == gesVersion {
			claimed = append(claimed, name)
		}
	}
	if len(claimed) != 1 || claimed[0] != gesUp {
		t.Errorf("forward migrations claiming %s = %v, want exactly [%s]: the migrator would apply one and record the other as applied", gesVersion, claimed, gesUp)
	}
	if gesVersion <= cevVersion {
		t.Errorf("%s must sort after the usage ledger migration %s (it is the one CR-T12 was told to follow)", gesVersion, cevVersion)
	}
	if _, err := os.Stat(filepath.Join(migrationsDir(t), gesDown)); err != nil {
		t.Errorf("the migrator refuses to roll back a migration without a paired .down.sql: %v", err)
	}
	// init.sql is not the convention for a new table: the migration is the only definition,
	// so a fresh database and a live one cannot disagree about it.
	if strings.Contains(readInit(t), "graph_extraction_state") {
		t.Error("init.sql defines graph_extraction_state: a new table lives in its migration only (mirroring is for cost_events)")
	}
}

func TestGraphExtractionStateUpIsIdempotentByConstruction(t *testing.T) {
	sql := code(readSQL(t, gesUp))
	if tx := regexp.MustCompile(`(?im)^\s*(BEGIN|COMMIT|ROLLBACK|START\s+TRANSACTION)\s*;`).FindString(sql); tx != "" {
		t.Errorf("migration contains transaction control %q; the migrator owns the transaction", strings.TrimSpace(tx))
	}
	if strings.Contains(strings.ToUpper(sql), "CONCURRENTLY") {
		t.Error("CREATE INDEX CONCURRENTLY cannot run inside the migrator's transaction")
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
	names := matches(regexp.MustCompile(`(?i)\bADD\s+CONSTRAINT\s+(\w+)`), sql)
	if len(names) != 8 {
		t.Errorf("the migration adds %d named constraints, want 8: %v", len(names), names)
	}
	for _, name := range names {
		drop := regexp.MustCompile(`(?i)DROP\s+CONSTRAINT\s+IF\s+EXISTS\s+` + regexp.QuoteMeta(name) + `\b`)
		if !drop.MatchString(sql) {
			t.Errorf("ADD CONSTRAINT %s is not preceded by DROP CONSTRAINT IF EXISTS %s, so a second run fails with duplicate_object", name, name)
		}
	}
	if !regexp.MustCompile(`(?i)DROP\s+POLICY\s+IF\s+EXISTS\s+graph_extraction_state_rls_isolation\b`).MatchString(sql) {
		t.Error("the policy is not dropped before it is created, so a second run fails with duplicate_object")
	}
	// The backfill inserts what is missing and never rewrites what graph-index has recorded.
	backfill := statementBody(t, sql, `INSERT INTO graph_extraction_state`, `;`)
	if !regexp.MustCompile(`(?is)ON\s+CONFLICT\s*\(\s*org_id\s*,\s*knowledge_id\s*\)\s+DO\s+NOTHING`).MatchString(backfill) {
		t.Error("the backfill does not end in ON CONFLICT (org_id, knowledge_id) DO NOTHING: a re-run would overwrite recorded state")
	}
}

func TestGraphExtractionStateDownReversesEverythingUpCreates(t *testing.T) {
	up, down := code(readSQL(t, gesUp)), code(readSQL(t, gesDown))
	for _, view := range matches(regexp.MustCompile(`(?i)\bCREATE\s+OR\s+REPLACE\s+VIEW\s+(\w+)`), up) {
		if !regexp.MustCompile(`(?i)DROP\s+VIEW\s+IF\s+EXISTS\s+` + view + `\b`).MatchString(down) {
			t.Errorf("rollback does not drop view %s", view)
		}
	}
	for _, table := range matches(regexp.MustCompile(`(?i)\bCREATE\s+TABLE\s+IF\s+NOT\s+EXISTS\s+(\w+)`), up) {
		if !regexp.MustCompile(`(?i)DROP\s+TABLE\s+IF\s+EXISTS\s+(public\.)?` + table + `\b`).MatchString(down) {
			t.Errorf("rollback does not drop table %s", table)
		}
	}
	// The view reads the table, so it has to go first.
	view := regexp.MustCompile(`(?i)DROP\s+VIEW`).FindStringIndex(down)
	table := regexp.MustCompile(`(?i)DROP\s+TABLE`).FindStringIndex(down)
	if view == nil || table == nil || view[0] > table[0] {
		t.Error("the rollback must drop the view before the table it reads")
	}
	// It never touches the graph itself or any table it did not create.
	for _, other := range []string{"graph_entities", "graph_relationships", "graph_claims", "graph_text_units", "graph_communities", "knowledge_units", "documents"} {
		if regexp.MustCompile(`(?i)\b` + other + `\b`).MatchString(down) {
			t.Errorf("the rollback names %s: it may only drop what the migration created", other)
		}
	}
}

func TestGraphExtractionStateMigrationBoundsItsLockWait(t *testing.T) {
	for _, name := range []string{gesUp} {
		sql := code(readSQL(t, name))
		lock := regexp.MustCompile(`(?i)SET\s+LOCAL\s+lock_timeout\s*=`).FindStringIndex(sql)
		first := regexp.MustCompile(`(?i)\bCREATE\s+TABLE\b`).FindStringIndex(sql)
		if lock == nil {
			t.Errorf("%s does not set a lock_timeout", name)
			continue
		}
		if first == nil || lock[0] > first[0] {
			t.Errorf("%s: lock_timeout must be set before the first statement that touches a table", name)
		}
	}
}

// The vocabularies are closed and pinned: graph-index writes the status and the error class
// from enums, and a value the CHECK does not know turns a typed outcome into a failed write.
// The pins below are read from the Rust source, so the two cannot drift.
func TestGraphExtractionStateVocabulariesArePinnedAndMatchTheCode(t *testing.T) {
	sql := code(readSQL(t, gesUp))
	wantStatuses := []string{
		"pending", "ready", "empty",
		"skipped_zdr", "skipped_policy", "skipped_visibility", "skipped_budget",
		"failed_transient", "failed_permanent",
	}
	wantClasses := []string{
		"rate_limited", "provider_unavailable", "timeout", "auth",
		"zdr_unavailable", "zdr_org", "zdr_document",
		"residency_unavailable", "posture_unavailable",
		"invalid_output", "too_long", "internal",
	}
	if got := quotedTokens(constraintCheckBody(t, sql, "graph_extraction_state_status_chk")); !sameSet(got, wantStatuses) {
		missing, extra := diffSets(wantStatuses, got)
		t.Errorf("status CHECK covers %v: missing %v, extra %v", got, missing, extra)
	}
	if got := quotedTokens(constraintCheckBody(t, sql, "graph_extraction_state_error_class_chk")); !sameSet(got, wantClasses) {
		missing, extra := diffSets(wantClasses, got)
		t.Errorf("error_class CHECK covers %v: missing %v, extra %v", got, missing, extra)
	}
	// The classes that must come with a failure or a zero-retention skip.
	required := quotedTokens(constraintCheckBody(t, sql, "graph_extraction_state_failure_class_chk"))
	if !sameSet(required, []string{"failed_transient", "failed_permanent", "skipped_zdr"}) {
		t.Errorf("the statuses that must carry an error class are %v, want failed_transient, failed_permanent and skipped_zdr", required)
	}

	rust := readSource(t, gesState)
	inCode := func(constant string) []string {
		m := regexp.MustCompile(`(?s)pub const ` + constant + `: \[&str; \d+\] = \[(.*?)\];`).FindStringSubmatch(rust)
		if m == nil {
			t.Fatalf("%s no longer defines %s", gesState, constant)
		}
		return matches(regexp.MustCompile(`"([a-z_]+)"`), m[1])
	}
	if got := inCode("STATUS_NAMES"); !sameSet(got, wantStatuses) {
		missing, extra := diffSets(wantStatuses, got)
		t.Errorf("graph-index writes statuses %v: the table does not accept %v and never receives %v", got, extra, missing)
	}
	if got := inCode("ERROR_CLASS_NAMES"); !sameSet(got, wantClasses) {
		missing, extra := diffSets(wantClasses, got)
		t.Errorf("graph-index writes error classes %v: the table does not accept %v and never receives %v", got, extra, missing)
	}

	// The legacy stamp the backfill writes is the one the code and the stamp CHECK know.
	m := regexp.MustCompile(`pub const LEGACY_PROMPT_VERSION: &str = "([a-z0-9_-]+)";`).FindStringSubmatch(rust)
	if m == nil {
		t.Fatalf("%s no longer defines LEGACY_PROMPT_VERSION", gesState)
	}
	for _, place := range []string{
		statementBody(t, sql, `INSERT INTO graph_extraction_state`, `;`),
		constraintCheckBody(t, sql, "graph_extraction_state_stamp_chk"),
	} {
		if !strings.Contains(place, "'"+m[1]+"'") {
			t.Errorf("the migration no longer says %q where the code writes its legacy stamp", m[1])
		}
	}
}

func TestGraphExtractionStateTableAndViewAreRowLevelSecurityAware(t *testing.T) {
	sql := code(readSQL(t, gesUp))

	// Without security_invoker the view runs as its owner (a superuser, which bypasses RLS)
	// and a scoped dataplane_app read would return every org.
	if !regexp.MustCompile(`(?is)CREATE\s+OR\s+REPLACE\s+VIEW\s+v_graph_extraction_coverage\s+WITH\s*\(\s*security_invoker\s*=\s*true\s*\)`).MatchString(sql) {
		t.Error("v_graph_extraction_coverage must be created WITH (security_invoker = true)")
	}
	if !regexp.MustCompile(`(?is)GRANT\s+SELECT\s+ON\s+(public\.)?v_graph_extraction_coverage\s+TO\s+dataplane_app`).MatchString(sql) {
		t.Error("the runtime role is not granted SELECT on v_graph_extraction_coverage")
	}
	if !regexp.MustCompile(`(?is)REVOKE\s+INSERT\s*,\s*UPDATE\s*,\s*DELETE\s+ON\s+(public\.)?v_graph_extraction_coverage\s+FROM\s+dataplane_app`).MatchString(sql) {
		t.Error("write privileges on v_graph_extraction_coverage are not revoked from the runtime role")
	}

	// A new org table is not protected automatically (20260809120000 says so).
	if !regexp.MustCompile(`(?is)ALTER\s+TABLE\s+(public\.)?graph_extraction_state\s+ENABLE\s+ROW\s+LEVEL\s+SECURITY`).MatchString(sql) {
		t.Error("graph_extraction_state does not ENABLE ROW LEVEL SECURITY")
	}
	policy := statementBody(t, sql, `CREATE POLICY graph_extraction_state_rls_isolation`, `;`)
	for _, clause := range []string{"USING", "WITH CHECK"} {
		if !strings.Contains(policy, clause+" (org_id = current_setting('app.current_org', true))") {
			t.Errorf("graph_extraction_state policy is missing %s (org_id = current_setting('app.current_org', true))", clause)
		}
	}
	if !regexp.MustCompile(`(?is)GRANT\s+SELECT\s*,\s*INSERT\s*,\s*UPDATE\s*,\s*DELETE\s+ON\s+(public\.)?graph_extraction_state\s+TO\s+dataplane_app`).MatchString(sql) {
		t.Error("the runtime role is not granted access to graph_extraction_state")
	}
	// The key is the organization and the chunk, and the chunk cascades with its unit.
	if !regexp.MustCompile(`(?is)PRIMARY\s+KEY\s*\(\s*org_id\s*,\s*knowledge_id\s*\)`).MatchString(sql) {
		t.Error("the primary key is not (org_id, knowledge_id)")
	}
	if !regexp.MustCompile(`(?is)knowledge_id\s+TEXT\s+NOT\s+NULL\s+REFERENCES\s+knowledge_units\s*\(\s*knowledge_id\s*\)\s+ON\s+DELETE\s+CASCADE`).MatchString(sql) {
		t.Error("knowledge_id does not reference knowledge_units with ON DELETE CASCADE: a re-chunk would leave state for chunks that no longer exist")
	}
}

func TestGraphExtractionStateMigrationVerifiesItsOwnBackfill(t *testing.T) {
	sql := code(readSQL(t, gesUp))
	if !regexp.MustCompile(`DO\s+\$post\$`).MatchString(sql) {
		t.Fatal("migration has no post-condition block (DO $post$ ... $post$)")
	}
	block := statementBody(t, sql, `DO\s+\$post\$`, `\$post\$;`)
	if !strings.Contains(block, "RAISE EXCEPTION") {
		t.Error("the post-condition block must raise when a chunk with graph rows is left without a state")
	}
	insert := regexp.MustCompile(`INSERT\s+INTO\s+graph_extraction_state`).FindStringIndex(sql)
	post := regexp.MustCompile(`DO\s+\$post\$`).FindStringIndex(sql)
	if insert == nil || post == nil || post[0] < insert[0] {
		t.Error("the post-condition must come after the backfill")
	}
}

// The backfill records two facts and invents nothing: a chunk with graph rows was extracted
// (ready), by a model nobody recorded (no model, no provider), under a prompt nobody versioned
// (legacy). A chunk without rows is not recorded at all: "tried and found nothing" cannot be
// told from "never tried" by anything in the database.
func TestGraphExtractionStateBackfillNeverFabricatesProvenance(t *testing.T) {
	sql := code(readSQL(t, gesUp))
	insert := statementBody(t, sql, `INSERT INTO graph_extraction_state`, `;`)
	for _, forbidden := range []string{"model", "provider", "error_class", "next_attempt_at", "lease_until"} {
		if regexp.MustCompile(`(?i)\b` + forbidden + `\b`).MatchString(strings.SplitN(insert, "FROM", 2)[0]) {
			t.Errorf("the backfill assigns %s: it may only record that the chunk was extracted", forbidden)
		}
	}
	for _, want := range []string{"'ready'", "'legacy'", "graph_text_units", "JOIN knowledge_units"} {
		if !strings.Contains(insert, want) {
			t.Errorf("the backfill no longer contains %q", want)
		}
	}
	if strings.Contains(insert, "'empty'") {
		t.Error("the backfill invents empty rows: nothing in the database says a chunk was tried and found nothing")
	}
}
