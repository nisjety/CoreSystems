package postgrescontract

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// The usage ledger migration (design task CR-T10). Like migration_static_test.go these read
// files and need no database, so they run everywhere, always. They pin the rules the consumer,
// the forwarder and cost-core's budget exclusion are written against, and they keep the
// three places the cost_events DDL lives (the migration, its rollback and init.sql) saying
// the same thing.

const (
	cevVersion = "20261003140000"
	cevUp      = "20261003140000_cost_events_v2.sql"
	cevDown    = "20261003140000_cost_events_v2.down.sql"
)

// v1Columns are the columns cost_events had before this migration (the two older migrations
// and init.sql): what the consumer's INSERT may name besides the ones this one adds.
var v1Columns = []string{"id", "event_type", "model", "org_id", "count", "estimated_tokens", "idempotency_key", "created_at", "user_id"}

func readInit(t testing.TB) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(dataPlaneRoot(t), "infra", "postgres", "init.sql"))
	if err != nil {
		t.Fatalf("read init.sql: %v", err)
	}
	return strings.ReplaceAll(string(raw), "\r\n", "\n")
}

func readSource(t testing.TB, relative string) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(dataPlaneRoot(t), filepath.FromSlash(relative)))
	if err != nil {
		t.Fatalf("read %s: %v", relative, err)
	}
	return strings.ReplaceAll(string(raw), "\r\n", "\n")
}

func TestCostEventsMigrationIsAPairAfterContextProvenanceWithAFreeVersion(t *testing.T) {
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
		if m := forward.FindStringSubmatch(name); m != nil && m[1] == cevVersion {
			claimed = append(claimed, name)
		}
	}
	if len(claimed) != 1 || claimed[0] != cevUp {
		t.Errorf("forward migrations claiming %s = %v, want exactly [%s]: the migrator would apply one and record the other as applied", cevVersion, claimed, cevUp)
	}
	if cevVersion <= cprVersion {
		t.Errorf("%s must sort after the context provenance migration %s", cevVersion, cprVersion)
	}
	if _, err := os.Stat(filepath.Join(migrationsDir(t), cevDown)); err != nil {
		t.Errorf("the migrator refuses to roll back a migration without a paired .down.sql: %v", err)
	}
}

func TestCostEventsUpIsIdempotentByConstruction(t *testing.T) {
	sql := code(readSQL(t, cevUp))
	if tx := regexp.MustCompile(`(?im)^\s*(BEGIN|COMMIT|ROLLBACK|START\s+TRANSACTION)\s*;`).FindString(sql); tx != "" {
		t.Errorf("migration contains transaction control %q; the migrator owns the transaction", strings.TrimSpace(tx))
	}
	if strings.Contains(strings.ToUpper(sql), "CONCURRENTLY") {
		t.Error("CREATE INDEX CONCURRENTLY cannot run inside the migrator's transaction")
	}
	columns := matches(regexp.MustCompile(`(?is)\bADD\s+COLUMN\s+(?:IF\s+NOT\s+EXISTS\s+)?(\w+)`), sql)
	if len(columns) != 16 {
		t.Errorf("the migration adds %d columns, want 16: %v", len(columns), columns)
	}
	for _, m := range regexp.MustCompile(`(?i)\bADD\s+COLUMN\s+(IF\s+NOT\s+EXISTS\s+)?(\w+)`).FindAllStringSubmatch(sql, -1) {
		if m[1] == "" {
			t.Errorf("ADD COLUMN %s lacks IF NOT EXISTS", m[2])
		}
	}
	for _, m := range regexp.MustCompile(`(?i)\bCREATE\s+(?:UNIQUE\s+)?INDEX\s+(IF\s+NOT\s+EXISTS\s+)?(\w+)`).FindAllStringSubmatch(sql, -1) {
		if m[1] == "" {
			t.Errorf("CREATE INDEX %s lacks IF NOT EXISTS", m[2])
		}
	}
	for _, name := range matches(regexp.MustCompile(`(?i)\bADD\s+CONSTRAINT\s+(\w+)`), sql) {
		drop := regexp.MustCompile(`(?i)DROP\s+CONSTRAINT\s+IF\s+EXISTS\s+` + regexp.QuoteMeta(name) + `\b`)
		if !drop.MatchString(sql) {
			t.Errorf("ADD CONSTRAINT %s is not preceded by DROP CONSTRAINT IF EXISTS %s, so a second run fails with duplicate_object", name, name)
		}
	}
	// The one data statement touches only rows that have no feature yet, and only version 1.
	update := statementBody(t, sql, `UPDATE cost_events SET feature`, `;`)
	for _, want := range []string{"feature IS NULL", "schema_version = 1"} {
		if !strings.Contains(update, want) {
			t.Errorf("the backfill UPDATE does not restrict itself with %q, so a second run (or a later one) would rewrite rows it should not", want)
		}
	}
}

func TestCostEventsDownReversesEverythingUpCreates(t *testing.T) {
	up, down := code(readSQL(t, cevUp)), code(readSQL(t, cevDown))
	for _, column := range matches(regexp.MustCompile(`(?i)\bADD\s+COLUMN\s+IF\s+NOT\s+EXISTS\s+(\w+)`), up) {
		if !regexp.MustCompile(`(?i)DROP\s+COLUMN\s+IF\s+EXISTS\s+` + column + `\b`).MatchString(down) {
			t.Errorf("rollback does not drop column %s", column)
		}
	}
	for _, name := range matches(regexp.MustCompile(`(?i)\bADD\s+CONSTRAINT\s+(\w+)`), up) {
		if !regexp.MustCompile(`(?i)DROP\s+CONSTRAINT\s+IF\s+EXISTS\s+` + name + `\b`).MatchString(down) {
			t.Errorf("rollback does not drop constraint %s", name)
		}
	}
	for _, name := range matches(regexp.MustCompile(`(?i)\bCREATE\s+INDEX\s+IF\s+NOT\s+EXISTS\s+(\w+)`), up) {
		if !regexp.MustCompile(`(?i)DROP\s+INDEX\s+IF\s+EXISTS\s+` + name + `\b`).MatchString(down) {
			t.Errorf("rollback does not drop index %s", name)
		}
	}
	// The rollback never deletes the ledger's rows: it discards what this migration added to
	// them, which the file says in its header, and nothing else.
	if regexp.MustCompile(`(?i)\b(DELETE\s+FROM|TRUNCATE|DROP\s+TABLE)\b`).MatchString(down) {
		t.Error("the rollback deletes rows or drops the table: it must only drop what the migration added")
	}
}

func TestCostEventsMigrationsBoundTheirLockWaitBeforeTouchingTheTable(t *testing.T) {
	for _, name := range []string{cevUp, cevDown} {
		sql := code(readSQL(t, name))
		lock := regexp.MustCompile(`(?i)SET\s+LOCAL\s+lock_timeout\s*=`).FindStringIndex(sql)
		alter := regexp.MustCompile(`(?i)\bALTER\s+TABLE\b`).FindStringIndex(sql)
		if lock == nil {
			t.Errorf("%s does not set a lock_timeout", name)
			continue
		}
		if alter == nil || lock[0] > alter[0] {
			t.Errorf("%s: lock_timeout must be set before the first ALTER TABLE", name)
		}
	}
}

// The vocabularies are closed and pinned: the consumer writes the cost basis, the forwarder
// writes the failure class, and a value the CHECK does not know turns a typed outcome into a
// failed write.
func TestCostEventsVocabulariesArePinnedAndMatchTheCode(t *testing.T) {
	sql := code(readSQL(t, cevUp))
	basis := quotedTokens(constraintCheckBody(t, sql, "cost_events_cost_basis_chk"))
	if !sameSet(basis, []string{"metered", "estimated"}) {
		t.Errorf("cost_basis CHECK covers %v, want metered and estimated", basis)
	}
	failures := quotedTokens(constraintCheckBody(t, sql, "cost_events_forward_state_chk"))
	wantFailures := []string{"rejected", "unauthorized", "unavailable", "token_unavailable"}
	if !sameSet(failures, wantFailures) {
		t.Errorf("forward_error CHECK covers %v, want %v", failures, wantFailures)
	}

	// The forwarder's constants are the same words.
	forwarder := readSource(t, "services/data-orchestrator-go/internal/cost/forwarder.go")
	var inCode []string
	for _, name := range []string{"failRejected", "failUnauthorized", "failUnavailable", "failTokenUnavailable"} {
		m := regexp.MustCompile(name + `\s*=\s*"([a-z_]+)"`).FindStringSubmatch(forwarder)
		if m == nil {
			t.Fatalf("forwarder.go no longer defines %s", name)
		}
		inCode = append(inCode, m[1])
	}
	if !sameSet(inCode, wantFailures) {
		t.Errorf("the forwarder writes failure classes %v, the table accepts %v", inCode, wantFailures)
	}

	// The consumer only writes a basis the table accepts.
	verifier := readSource(t, "services/data-orchestrator-go/internal/cost/verifier.go")
	if !strings.Contains(verifier, `decoded.CostBasis != "metered" && decoded.CostBasis != "estimated"`) {
		t.Error("the verifier no longer restricts cost_basis to metered and estimated")
	}
}

// The consumer's INSERT only names columns that exist, with one placeholder each.
func TestTheConsumersInsertNamesOnlyColumnsTheTableHas(t *testing.T) {
	consumer := readSource(t, "services/data-orchestrator-go/internal/cost/consumer.go")
	m := regexp.MustCompile("(?s)insertCostEventSQL = `\\s*INSERT INTO cost_events \\((.*?)\\)\\s*VALUES \\((.*?)\\)\\s*ON CONFLICT").FindStringSubmatch(consumer)
	if m == nil {
		t.Fatal("cannot find insertCostEventSQL in consumer.go")
	}
	var written []string
	for _, column := range strings.Split(m[1], ",") {
		written = append(written, strings.TrimSpace(column))
	}
	placeholders := strings.Split(m[2], ",")
	if len(placeholders) != len(written) {
		t.Fatalf("the INSERT names %d columns and %d placeholders", len(written), len(placeholders))
	}
	known := append([]string{}, v1Columns...)
	known = append(known, matches(regexp.MustCompile(`(?i)\bADD\s+COLUMN\s+IF\s+NOT\s+EXISTS\s+(\w+)`), code(readSQL(t, cevUp)))...)
	for _, column := range written {
		if !contains(known, column) {
			t.Errorf("the consumer writes %q, which cost_events does not have", column)
		}
	}
	// The unique index it conflicts on is the one the table has.
	for _, want := range []string{"ON CONFLICT (idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING"} {
		if !strings.Contains(consumer, want) {
			t.Errorf("consumer.go lost %q: a redelivered event would be written twice", want)
		}
	}
	// The forwarder's claim only reads columns the table has.
	forwarder := readSource(t, "services/data-orchestrator-go/internal/cost/forwarder.go")
	claim := regexp.MustCompile("(?s)claimUsageRowsSQL = `(.*?)`").FindStringSubmatch(forwarder)
	if claim == nil {
		t.Fatal("cannot find claimUsageRowsSQL in forwarder.go")
	}
	for _, column := range matches(regexp.MustCompile(`\be\.(\w+)`), claim[1]) {
		if !contains(known, column) {
			t.Errorf("the claim reads e.%s, which cost_events does not have", column)
		}
	}
}

// cost_events DDL lives in init.sql too (the baseline of a fresh database). The mirror carries
// every column, every constraint (body for body) and every index of the migration.
func TestInitSQLMirrorsTheCostEventsMigration(t *testing.T) {
	initSQL := readInit(t)
	up := code(readSQL(t, cevUp))
	at := strings.Index(initSQL, "CR-T10 — usage ledger v2")
	if at < 0 {
		t.Fatal("init.sql has no CR-T10 mirror block")
	}
	mirror := initSQL[at:]
	for _, column := range matches(regexp.MustCompile(`(?i)\bADD\s+COLUMN\s+IF\s+NOT\s+EXISTS\s+(\w+)`), up) {
		if !regexp.MustCompile(`(?i)\bADD\s+COLUMN\s+IF\s+NOT\s+EXISTS\s+` + column + `\b`).MatchString(mirror) {
			t.Errorf("init.sql does not add column %s", column)
		}
	}
	normalize := func(s string) string { return strings.Join(strings.Fields(s), " ") }
	for _, name := range matches(regexp.MustCompile(`(?i)\bADD\s+CONSTRAINT\s+(\w+)`), up) {
		want := normalize(constraintCheckBody(t, up, name))
		got := normalize(constraintCheckBody(t, code(mirror), name))
		if got != want {
			t.Errorf("init.sql constraint %s differs from the migration:\n  migration: %s\n  init.sql:  %s", name, want, got)
		}
	}
	for _, name := range matches(regexp.MustCompile(`(?i)\bCREATE\s+INDEX\s+IF\s+NOT\s+EXISTS\s+(\w+)`), up) {
		want := normalize(statementBody(t, up, `CREATE INDEX IF NOT EXISTS `+name, `;`))
		got := normalize(statementBody(t, code(mirror), `CREATE INDEX IF NOT EXISTS `+name, `;`))
		if got != want {
			t.Errorf("init.sql index %s differs from the migration:\n  migration: %s\n  init.sql:  %s", name, want, got)
		}
	}
}

// The consumer and the forwarder rely on the columns being nullable where a version 1 row has
// nothing: a NOT NULL on an exact column would make the old consumer's INSERT fail.
func TestOnlyCountersWithConstantDefaultsAreNotNull(t *testing.T) {
	up := code(readSQL(t, cevUp))
	for _, m := range regexp.MustCompile(`(?i)ADD\s+COLUMN\s+IF\s+NOT\s+EXISTS\s+(\w+)\s+([^,;]*)`).FindAllStringSubmatch(up, -1) {
		name, definition := m[1], strings.ToUpper(m[2])
		if strings.Contains(definition, "NOT NULL") && !strings.Contains(definition, "DEFAULT") {
			t.Errorf("column %s is NOT NULL with no default: the running consumer's INSERT (which names the seven old columns only) would fail", name)
		}
	}
}
