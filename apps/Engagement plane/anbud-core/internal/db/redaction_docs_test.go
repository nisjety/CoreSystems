package db_test

import (
	"regexp"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The runbook docs/PERSONAL-DATA.md is read as text, so that it cannot drift from the
// migration it describes without a pure test failing, and its SQL blocks are run against a
// server by TestRunbookSQLRuns (redaction_runbook_db_test.go).

const personalDataDoc = "docs/PERSONAL-DATA.md"

// runbookBlocks returns the SQL blocks of the runbook by label. A block is a ```sql fence
// with a line `<!-- runbook-sql: label -->` directly above it.
func runbookBlocks(t testing.TB) map[string]string {
	t.Helper()
	doc := dbtest.ReadModuleFile(t, personalDataDoc)
	fence := regexp.MustCompile("(?s)<!-- runbook-sql: ([a-z-]+) -->\n```sql\n(.*?)```")
	blocks := map[string]string{}
	for _, m := range fence.FindAllStringSubmatch(doc, -1) {
		if _, dup := blocks[m[1]]; dup {
			t.Fatalf("the runbook has two SQL blocks labelled %q", m[1])
		}
		blocks[m[1]] = m[2]
	}
	return blocks
}

// The runbook says what it is: a recommendation awaiting a compliance check, not advice,
// and it lists the questions that are open.
func TestPersonalDataRunbookStatesItsLimits(t *testing.T) {
	t.Parallel()

	doc := strings.ToLower(dbtest.ReadModuleFile(t, personalDataDoc))
	for _, want := range []string{
		"not legal advice",
		"recommendation",
		"awaiting a compliance check",
		"open compliance questions",
		// the open questions the owner asked it to list
		"lawful basis", "balancing test", "transparency", "retention", "response times",
		"sole trader", "backups", "standing requests", "controller",
		// what it must say about Data Plane v2 and a restore
		"nothing is published from this core to data plane v2",
		"the authoritative list of requests is the ticket system",
	} {
		if !strings.Contains(doc, want) {
			t.Errorf("the runbook does not contain %q", want)
		}
	}
}

// Every SQL block the steps rely on is there, once, and none still holds the real thing
// where a placeholder belongs.
func TestPersonalDataRunbookHasEveryBlockTheStepsNeed(t *testing.T) {
	t.Parallel()

	blocks := runbookBlocks(t)
	for _, label := range []string{"whoami", "logging", "find", "find-other", "redact", "commit", "redact-many", "restore-find", "restore-check"} {
		if strings.TrimSpace(blocks[label]) == "" {
			t.Errorf("the runbook has no SQL block labelled %q", label)
		}
	}
	placeholder := regexp.MustCompile(`<[A-Z][A-Z0-9_]*>`)
	allowed := []string{"<IDENTIFIER>", "<ID>", "<ID2>", "<REASON>", "<TICKET>", "<DOFFIN_ID>", "<ORIGINAL_SHA256>"}
	for label, sql := range blocks {
		for _, p := range placeholder.FindAllString(sql, -1) {
			if !slices.Contains(allowed, p) {
				t.Errorf("block %q has the placeholder %s, which the runbook test does not fill in", label, p)
			}
		}
	}
	if find := blocks["find"]; !strings.Contains(find, "position(convert_to('<IDENTIFIER>', 'UTF8') in body)") {
		t.Error("the find block does not search the body bytes with position(convert_to(..., 'UTF8') in body)")
	}
	if strings.Contains(strings.ToLower(blocks["find"]), "select body") || strings.Contains(strings.ToLower(blocks["find"]), ", body") {
		t.Error("the find block returns bodies; it must return ids only")
	}
}

// What the runbook says about the mechanism is what the migration built: the reasons, the
// SQLSTATEs the function raises, and the names of the objects.
func TestPersonalDataRunbookAgreesWithTheMigration(t *testing.T) {
	t.Parallel()

	doc := dbtest.ReadModuleFile(t, personalDataDoc)
	migration := redactionMigration(t)

	// The reason table lists the reasons of the tombstone's check, no more and no fewer.
	start := strings.Index(doc, "| Reason | Use it when |")
	end := strings.Index(doc, "The erasure cannot be undone.")
	if start < 0 || end < start {
		t.Fatal("the runbook has no reason table before 'The erasure cannot be undone.'")
	}
	var reasons []string
	for _, m := range regexp.MustCompile("(?m)^\\| `([a-z_]+)` \\|").FindAllStringSubmatch(doc[start:end], -1) {
		reasons = append(reasons, m[1])
	}
	slices.Sort(reasons)
	want := []string{"authority_order", "controller_decision", "erasure_request", "objection", "source_withdrawn"}
	if !slices.Equal(reasons, want) {
		t.Errorf("the runbook's reasons = %v, want the migration's %v", reasons, want)
	}
	for _, reason := range want {
		if !strings.Contains(migration, "'"+reason+"'") {
			t.Errorf("the migration does not know the reason %s", reason)
		}
	}

	// The refusals it lists are the ones the function raises.
	for state, condition := range map[string]string{
		"22004": "null_value_not_allowed",
		"22023": "invalid_parameter_value",
		"P0002": "no_data_found",
		"55000": "object_not_in_prerequisite_state",
	} {
		if !strings.Contains(doc, "| `"+state+"` |") {
			t.Errorf("the runbook's table of refusals has no row for %s", state)
		}
		if !strings.Contains(migration, "ERRCODE = '"+condition+"'") {
			t.Errorf("the migration does not raise %s (%s)", condition, state)
		}
	}

	// The objects it names exist in the migration.
	for _, object := range []string{
		"anbud_redact_raw_document", "raw_redactions", "raw_documents_live", "redacted_at",
		"raw_documents_change_guard", "raw_documents_insert_guard", "anbud.redacting_raw_document",
	} {
		if !strings.Contains(doc, object) {
			t.Errorf("the runbook does not mention %s", object)
		}
		if !strings.Contains(migration, object) {
			t.Errorf("the runbook names %s, which the migration does not create", object)
		}
	}
}

// A runbook for personal data must not hold any. It names no address and no person: the
// examples are placeholders.
func TestPersonalDataRunbookHoldsNoPersonalData(t *testing.T) {
	t.Parallel()

	doc := dbtest.ReadModuleFile(t, personalDataDoc)
	if m := regexp.MustCompile(`[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}`).FindString(doc); m != "" {
		t.Errorf("the runbook contains an e-mail address: %q", m)
	}
	if m := regexp.MustCompile(`\+\d{2}[ \d]{6,}`).FindString(doc); m != "" {
		t.Errorf("the runbook contains a telephone number: %q", m)
	}
}
