package gdpr

import (
	"regexp"
	"strings"
	"testing"
)

// The purger lists the tables it erases explicitly (see PostgresOrgPurger): a table that is not on
// the list survives an organization's erasure. org_processing_policy is org data, so an erased
// organization must not keep an operator's policy row (its reason and the operator's id) behind.
func TestTheOrgPurgeListsEveryTableThisServiceOwnsForAnOrganization(t *testing.T) {
	want := []string{"quality_eval_runs", "eval_golden_judgments", "org_processing_policy"}
	if len(purgeStatements) != len(want) {
		t.Fatalf("the purge erases %d tables, want %d (%v)", len(purgeStatements), len(want), want)
	}
	for i, table := range want {
		got := purgeStatements[i]
		if got.table != table {
			t.Errorf("statement %d erases %q, want %q", i, got.table, table)
		}
		// Static, org-scoped and nothing else: no code path can filter on anything but the one bound org id.
		pattern := regexp.MustCompile(`^DELETE FROM ` + regexp.QuoteMeta(table) + ` WHERE org_id = \$1$`)
		if !pattern.MatchString(got.sql) {
			t.Errorf("statement for %s is %q: it must be a static DELETE ... WHERE org_id = $1", table, got.sql)
		}
	}
}

// The integration test builds the policy table from the migration's own DDL; if the migration
// stops defining it in the shape the extraction expects, that must fail here, with no database,
// rather than skip.
func TestTheProcessingPolicyDDLIsTakenFromTheMigration(t *testing.T) {
	ddl := processingPolicyDDL(t)
	for _, want := range []string{
		"CREATE TABLE IF NOT EXISTS org_processing_policy (",
		"org_id                 TEXT        PRIMARY KEY",
		"org_processing_policy_off_reason_chk",
	} {
		if !strings.Contains(ddl, want) {
			t.Errorf("the extracted DDL lacks %q:\n%s", want, ddl)
		}
	}
	if strings.Count(ddl, "CREATE TABLE") != 1 {
		t.Errorf("the extraction spans more than the one table:\n%s", ddl)
	}
}
