package api

import (
	"bytes"
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
)

// scopeBundleDB is the database a bundle route runs against under SKILL_BUNDLE_SCOPE_RULES. Its reads record their
// statements and arguments; the skill existence check answers skillExists. A bundle write must run in tx, and a nil
// tx means no write is expected, so any write fails the test.
type scopeBundleDB struct {
	t           *testing.T
	tx          *bundleTestRecorder
	skillExists bool
	queries     []databaseCall
}

func (d *scopeBundleDB) Begin(context.Context) (pgx.Tx, error) {
	if d.tx == nil {
		d.t.Fatal("the registry was written for a request that had to be refused")
	}
	return d.tx, nil
}

func (d *scopeBundleDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Fatal("bundle writes must run in a transaction")
	return pgconn.CommandTag{}, nil
}

func (d *scopeBundleDB) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	d.queries = append(d.queries, databaseCall{query: query, args: append([]any(nil), args...)})
	return emptyRows{}, nil
}

func (d *scopeBundleDB) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	d.queries = append(d.queries, databaseCall{query: query, args: append([]any(nil), args...)})
	if strings.Contains(query, "SELECT EXISTS (SELECT 1 FROM agent_skills WHERE id = $1 AND org_id = $2)") {
		return boolRow{value: d.skillExists}
	}
	d.t.Fatalf("unexpected read in a bundle route: %s", query)
	return errorRow{err: pgx.ErrNoRows}
}

// serveScopedBundle answers one request on a bundle route built the way cmd/main.go builds it, with
// SKILL_BUNDLES_ENABLED = enabled and SKILL_BUNDLE_SCOPE_RULES = scopeRules.
func serveScopedBundle(t *testing.T, enabled, scopeRules bool, db skillbundle.DB, principal *authctx.Principal, method, target string, body []byte) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewSkillBundlesHandler(db, enabled).WithScopeRules(scopeRules).Register(mux)
	request := httptest.NewRequest(method, target, bytes.NewReader(body))
	if principal != nil {
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), *principal))
	}
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

// Under SKILL_BUNDLE_SCOPE_RULES an upload needs its skill: a skill the org does not hold is a 404 skill_not_found and
// nothing is parsed or written. A skill the org holds takes the upload as before.
func TestSkillBundleUploadNeedsItsSkillUnderTheScopeRules(t *testing.T) {
	admin := bundleTestAdmin()
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})

	missing := &scopeBundleDB{t: t}
	response := serveScopedBundle(t, true, true, missing, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if response.Code != http.StatusNotFound {
		t.Fatalf("upload for a missing skill = %d, want 404 (%s)", response.Code, response.Body.String())
	}
	if got := orgAgentErrorCode(t, response); got != "skill_not_found" {
		t.Fatalf("error code = %q, want skill_not_found", got)
	}

	tx := &bundleTestRecorder{t: t}
	present := &scopeBundleDB{t: t, tx: tx, skillExists: true}
	response = serveScopedBundle(t, true, true, present, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if response.Code != http.StatusCreated {
		t.Fatalf("upload for an existing skill = %d, want 201 (%s)", response.Code, response.Body.String())
	}
	if !tx.committed {
		t.Fatal("the upload for an existing skill did not commit")
	}
}

// With SKILL_BUNDLE_SCOPE_RULES off, the upload is as it was: no existence check runs, so a skill row is not needed.
func TestSkillBundleUploadIsUnchangedWithTheScopeRulesOff(t *testing.T) {
	admin := bundleTestAdmin()
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	tx := &bundleTestRecorder{t: t}
	db := &scopeBundleDB{t: t, tx: tx, skillExists: false}
	response := serveScopedBundle(t, true, false, db, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if response.Code != http.StatusCreated {
		t.Fatalf("upload with the scope rules off = %d, want 201 (%s)", response.Code, response.Body.String())
	}
	for _, call := range db.queries {
		if strings.Contains(call.query, "FROM agent_skills WHERE id = $1") {
			t.Fatalf("the skill check ran with the scope rules off: %s", call.query)
		}
	}
}

// Under SKILL_BUNDLE_SCOPE_RULES a person's manifest is read with the skill visibility rule, as the caller and whether
// the caller is an admin, the verified scopes deciding it. A skill the person may not see answers the same body as a
// missing bundle.
func TestSkillBundleManifestFollowsTheSkillVisibility(t *testing.T) {
	admin := bundleTestAdmin()
	db := &scopeBundleDB{t: t}
	response := serveScopedBundle(t, true, true, db, &admin, http.MethodGet, "/api/v1/skills/skill_1/bundle", nil)
	if response.Code != http.StatusNotFound || !strings.Contains(response.Body.String(), "skill bundle not found") {
		t.Fatalf("invisible manifest = %d %s, want the missing-bundle not found", response.Code, response.Body.String())
	}
	read := db.queries[len(db.queries)-1]
	if !strings.Contains(read.query, "FROM agent_skills s") {
		t.Fatalf("the manifest read does not apply the skill visibility rule: %s", read.query)
	}
	if read.args[2] != "user-admin" || read.args[3] != true {
		t.Fatalf("visibility args = %#v, want the caller and the admin flag from the verified scopes", read.args)
	}

	missing := serveScopedBundle(t, true, false, &scopeBundleDB{t: t}, &admin, http.MethodGet, "/api/v1/skills/skill_1/bundle", nil)
	if missing.Code != response.Code || missing.Body.String() != response.Body.String() {
		t.Fatalf("invisible answered %d %q, a missing bundle %d %q; they must match", response.Code, response.Body.String(), missing.Code, missing.Body.String())
	}

	// A service with the admin scope keeps the unfiltered manifest read.
	service := bundleTestAdmin()
	service.PrincipalType = "service"
	service.ActorID = "execution-core"
	serviceDB := &scopeBundleDB{t: t}
	serveScopedBundle(t, true, true, serviceDB, &service, http.MethodGet, "/api/v1/skills/skill_1/bundle", nil)
	for _, call := range serviceDB.queries {
		if strings.Contains(call.query, "FROM agent_skills s") {
			t.Fatalf("a service's manifest read was narrowed by the visibility rule: %s", call.query)
		}
	}
}

// A skill delete removes the skill's bundles whenever the bundle surface is on under SKILL_BUNDLE_SCOPE_RULES, without
// retention (register D2-382). With either switch off the bundles stay, as before.
func TestSkillDeleteRemovesBundlesUnderTheScopeRules(t *testing.T) {
	cases := []struct {
		name                       string
		scopeRules, bundlesEnabled bool
		want                       bool
	}{
		{name: "scope rules and the bundle surface on", scopeRules: true, bundlesEnabled: true, want: true},
		{name: "the bundle surface off", scopeRules: true, bundlesEnabled: false, want: false},
		{name: "scope rules off", scopeRules: false, bundlesEnabled: true, want: false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			database := &recordingDatabase{}
			skills := NewSkillsHandler(database).WithBundleScopeRules(tc.scopeRules, tc.bundlesEnabled)
			response := serveSkillsHandler(t, skills, writeToken, http.MethodDelete, "/api/v1/skills/sk-1")
			if response.Code != http.StatusNoContent {
				t.Fatalf("delete status = %d, want 204 (%s)", response.Code, response.Body.String())
			}
			removed := false
			for _, call := range database.execs {
				if strings.Contains(call.query, "DELETE FROM skill_bundles") {
					removed = true
				}
			}
			if removed != tc.want {
				t.Fatalf("bundles removed = %v, want %v", removed, tc.want)
			}
		})
	}
}

// serveSkillsHandler answers one request on the skills routes of the given handler, behind the test authenticator.
func serveSkillsHandler(t *testing.T, skills *SkillsHandler, token func(sign mcpTokenSigner) string, method, target string) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	request := httptest.NewRequest(method, target, nil)
	request.Header.Set("Authorization", "Bearer "+token(sign))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}
