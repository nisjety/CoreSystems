package db_test

import (
	"regexp"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
)

// tenant_privileges.sql is the application's privileges on the schema tenant. Like privileges.sql it is
// read as text, and what it may grant is pinned: exactly four statements. A fifth, or a changed one, is a
// change to what an application with a stolen connection can do to a tenant's uploads, and it must be
// made on purpose, here, in the open. What it takes away is checked to be there too, because that is the
// half that keeps a grant made by hand from lasting.

var grantStatement = regexp.MustCompile(`(?is)\bgrant\b[^;]*;`)

// tenantStatement is a grant or a revoke: migration 0007 may have one revoke and no grant.
var tenantStatement = regexp.MustCompile(`(?is)\b(?:grant|revoke)\b[^;]*;`)

// allowedTenantGrants are the grants of the file, normalised: lower case, one space between words. The
// application inserts every column but id and created_at, which are the database's own.
var allowedTenantGrants = []string{
	"grant usage on schema tenant to anbud_app;",
	"grant select, delete on tenant.tender_uploads to anbud_app;",
	"grant select, insert, update on tenant.workspace_seen to anbud_app;",
	"grant insert (org_id, notice_id, uploaded_by, archive_name, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, findings_dropped, expires_at) on tenant.tender_uploads to anbud_app;",
	"grant execute on function tenant.purge_expired_uploads() to anbud_app;",
	"grant select, insert, update on tenant.workspace_profiles to anbud_app;",
	"grant select, insert, update, delete on tenant.pipeline_items to anbud_app;",
	"grant select, insert, delete on tenant.saved_searches to anbud_app;",
	"grant usage on sequence tenant.saved_searches_id_seq to anbud_app;",
}

// requiredTenantRevokes are what the file takes away at every start, as they are written in it.
var requiredTenantRevokes = []string{
	"revoke all on schema tenant from public;",
	"revoke all on schema tenant from anbud_dba;",
	"revoke all on table %s from public, anbud_app",
	"revoke all on table %s from anbud_dba",
	"revoke all on routine %s from public, anbud_app",
	"revoke all on routine %s from anbud_dba",
}

func normalise(s string) string { return strings.ToLower(strings.Join(strings.Fields(s), " ")) }

// tenantManifestProblems says what a tenant privileges script grants beyond the four statements, what it
// lacks of them, and which of the things it must take away it no longer says.
func tenantManifestProblems(script string) []string {
	var problems []string
	found := map[string]bool{}
	for _, st := range grantStatement.FindAllString(code(script), -1) {
		n := normalise(st)
		allowed := false
		for _, a := range allowedTenantGrants {
			if n == normalise(a) {
				allowed, found[a] = true, true
			}
		}
		if !allowed {
			problems = append(problems, "the tenant privileges script grants "+`"`+n+`"`+", which is not one of its four grants")
		}
	}
	for _, a := range allowedTenantGrants {
		if !found[a] {
			problems = append(problems, "the tenant privileges script no longer grants "+`"`+a+`"`)
		}
	}
	text := normalise(code(script))
	for _, r := range requiredTenantRevokes {
		if !strings.Contains(text, normalise(r)) {
			problems = append(problems, "the tenant privileges script no longer says "+`"`+r+`"`)
		}
	}
	return problems
}

func TestTenantPrivilegesManifestStaysWhereItIs(t *testing.T) {
	t.Parallel()

	if strings.TrimSpace(db.TenantPrivilegesSQL) == "" {
		t.Fatal("the embedded tenant privileges script is empty")
	}
	for _, problem := range tenantManifestProblems(db.TenantPrivilegesSQL) {
		t.Error(problem)
	}
}

// A checker that cannot fail proves nothing: it gets what a hurried change would add, and what a hurried
// change would remove.
func TestTenantPrivilegesManifestCheckerFlagsWhatItMustFlag(t *testing.T) {
	t.Parallel()

	whole := strings.Join(allowedTenantGrants, "\n") + "\n" + strings.Join(requiredTenantRevokes, ";\n") + ";"
	if problems := tenantManifestProblems(whole); len(problems) != 0 {
		t.Errorf("the checker flags the statements themselves: %v", problems)
	}
	for _, extra := range []string{
		`GRANT UPDATE ON tenant.tender_uploads TO anbud_app;`,
		`GRANT SELECT, INSERT, DELETE ON tenant.tender_uploads TO anbud_app;`,
		`GRANT SELECT, DELETE ON tenant.tender_uploads TO anbud_app WITH GRANT OPTION;`,
		`GRANT INSERT ON tenant.tender_uploads TO anbud_app;`,
		`GRANT INSERT (id) ON tenant.tender_uploads TO anbud_app;`,
		`GRANT INSERT (created_at) ON tenant.tender_uploads TO anbud_app;`,
		`GRANT TRUNCATE ON tenant.tender_uploads TO anbud_app;`,
		`GRANT ALL ON tenant.tender_uploads TO anbud_app;`,
		`GRANT SELECT ON tenant.tender_uploads TO PUBLIC;`,
		`GRANT SELECT ON tenant.tender_uploads TO anbud_dba;`,
		`GRANT USAGE ON SCHEMA tenant TO anbud_dba;`,
		`GRANT CREATE ON SCHEMA tenant TO anbud_app;`,
		`GRANT USAGE ON SCHEMA public TO anbud_app;`,
		`GRANT SELECT ON ALL TABLES IN SCHEMA tenant TO anbud_app;`,
		`GRANT EXECUTE ON FUNCTION tenant.something_else() TO anbud_app;`,
		`GRANT EXECUTE ON FUNCTION tenant.purge_expired_uploads() TO PUBLIC;`,
		`GRANT anbud_migrator TO anbud_app;`,
		`GRANT UPDATE (read_count) ON tenant.tender_uploads TO anbud_app;`,
	} {
		if problems := tenantManifestProblems(whole + "\n" + extra); len(problems) == 0 {
			t.Errorf("the checker does not flag %q", extra)
		}
	}
	// A grant that is gone is found too, and so is a revoke, and a grant inside the DO block is found like the rest.
	if problems := tenantManifestProblems(strings.Join(allowedTenantGrants[:3], "\n") + "\n" + strings.Join(requiredTenantRevokes, ";\n") + ";"); len(problems) == 0 {
		t.Error("the checker does not notice that a grant is missing")
	}
	for i := range requiredTenantRevokes {
		rest := append(append([]string{}, requiredTenantRevokes[:i]...), requiredTenantRevokes[i+1:]...)
		script := strings.Join(allowedTenantGrants, "\n") + "\n" + strings.Join(rest, ";\n") + ";"
		if problems := tenantManifestProblems(script); len(problems) == 0 {
			t.Errorf("the checker does not notice that %q is gone", requiredTenantRevokes[i])
		}
	}
	block := "DO $x$ BEGIN IF true THEN GRANT UPDATE ON tenant.tender_uploads TO anbud_app; END IF; END $x$;"
	if problems := tenantManifestProblems(whole + "\n" + block); len(problems) == 0 {
		t.Error("the checker does not flag a grant inside a DO block")
	}
}

// The table and the function the script names exist in a migration: a rename must fail here, in a pure
// test, and not at the first run against a database.
func TestTenantPrivilegesNameWhatMigration0007Creates(t *testing.T) {
	t.Parallel()

	_, body := db.EmbeddedMigration(t, 7)
	created := code(body)
	for _, want := range []string{"CREATE SCHEMA tenant", "CREATE TABLE tenant.tender_uploads", "CREATE FUNCTION tenant.purge_expired_uploads()"} {
		if !strings.Contains(created, want) {
			t.Errorf("migration 0007 no longer has %q, which the tenant privileges script grants on", want)
		}
	}
}

// Migration 0007 must not hand anyone a privilege itself: the grants are in tenant_privileges.sql, where
// the guard above is, and PUBLIC is refused the function.
func TestMigration0007GrantsNothing(t *testing.T) {
	t.Parallel()

	_, body := db.EmbeddedMigration(t, 7)
	for _, st := range tenantStatement.FindAllString(code(body), -1) {
		if n := normalise(st); n != "revoke all on function tenant.purge_expired_uploads() from public;" {
			t.Errorf("migration 0007 says %q: its privileges belong in tenant_privileges.sql", n)
		}
	}
	if !strings.Contains(normalise(code(body)), "revoke all on function tenant.purge_expired_uploads() from public;") {
		t.Error("migration 0007 no longer takes the function away from PUBLIC")
	}
}
