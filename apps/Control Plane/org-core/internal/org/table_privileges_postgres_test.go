package org

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// orgCoreAppTablePrivilegeNeeds is the set of privileges the org-core code runs
// under SET LOCAL ROLE org_core_app on each table migrations 021-023 create, in
// SQL order. Migration 024 grants exactly these. The test holds the database to
// them, so a table that picks up a privilege from the default privileges of
// migration 009, or a repository that starts using one without a grant, fails
// here rather than in production.
var orgCoreAppTablePrivilegeNeeds = []struct {
	table string
	need  string
}{
	{"org_model_policies", "SELECT,INSERT,DELETE"},
	{"org_model_policy_revisions", "SELECT,INSERT,UPDATE"},
	{"org_model_policy_history", "SELECT,INSERT"},
	{"siem_destinations", "SELECT,INSERT,UPDATE,DELETE"},
	{"organization_siem_audit_outbox", "SELECT,INSERT"},
	{"org_browser_site_policy", "SELECT,INSERT,UPDATE"},
	{"org_browser_site_rules", "SELECT,INSERT,DELETE"},
	{"user_browser_site_prefs", "SELECT,INSERT,UPDATE,DELETE"},
	{"org_browser_site_history", "SELECT,INSERT"},
}

// TestControlLifecycleOrgCoreAppTablePrivilegesMatchTheirCode applies the real
// org-core migrations to the disposable lifecycle database and compares what the
// runtime role can do on each table with orgCoreAppTablePrivilegeNeeds. Before
// migration 024 every one of these tables reports all four privileges: the default
// privileges of migration 009 apply to any table created after it, whatever GRANT
// the table's own migration lists.
func TestControlLifecycleOrgCoreAppTablePrivilegesMatchTheirCode(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	names := []string{"SELECT", "INSERT", "UPDATE", "DELETE"}
	for _, entry := range orgCoreAppTablePrivilegeNeeds {
		var granted [4]bool
		if err := db.Pool.QueryRow(ctx, `
SELECT has_table_privilege('org_core_app', $1::text, 'SELECT'),
       has_table_privilege('org_core_app', $1::text, 'INSERT'),
       has_table_privilege('org_core_app', $1::text, 'UPDATE'),
       has_table_privilege('org_core_app', $1::text, 'DELETE')`, entry.table,
		).Scan(&granted[0], &granted[1], &granted[2], &granted[3]); err != nil {
			t.Fatalf("inspect org_core_app privileges on %s: %v", entry.table, err)
		}
		var have []string
		for i, name := range names {
			if granted[i] {
				have = append(have, name)
			}
		}
		if got := strings.Join(have, ","); got != entry.need {
			t.Errorf("org_core_app holds %q on %s; the code needs %q", got, entry.table, entry.need)
		}
	}
}
