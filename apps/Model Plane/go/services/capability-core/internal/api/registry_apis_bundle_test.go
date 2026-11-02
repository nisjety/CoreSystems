package api

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
)

const bundleTestSkillMD = "---\nname: xlsx\ndescription: Spreadsheets\n---\nBody.\n"

func bundleTestZip(t *testing.T, entries map[string]string) []byte {
	t.Helper()
	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)
	for name, body := range entries {
		file, err := writer.Create(name)
		if err != nil {
			t.Fatalf("create %s: %v", name, err)
		}
		if _, err := file.Write([]byte(body)); err != nil {
			t.Fatalf("write %s: %v", name, err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatalf("close zip: %v", err)
	}
	return buffer.Bytes()
}

// bundleTestRecorder is the transaction a bundle write runs in. It answers the statements the
// store issues, records each one with its arguments, and fails the test on any statement the
// store should not issue.
type bundleTestRecorder struct {
	pgx.Tx
	t          *testing.T
	execs      []bundleTestExec
	deleteRows int64
	deleteHit  bool
	committed  bool
}

type bundleTestExec struct {
	sql  string
	args []any
}

func (tx *bundleTestRecorder) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	tx.execs = append(tx.execs, bundleTestExec{sql: sql, args: args})
	switch {
	case strings.Contains(sql, "DELETE FROM skill_bundles WHERE"):
		return pgconn.NewCommandTag("DELETE " + strconv.FormatInt(tx.deleteRows, 10)), nil
	default:
		return pgconn.NewCommandTag("OK 0"), nil
	}
}

func (tx *bundleTestRecorder) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	switch {
	case strings.Contains(sql, "INSERT INTO skill_bundles"):
		return bundleTestRow{values: []any{time.Date(2026, 10, 24, 9, 0, 0, 0, time.UTC)}}
	case strings.Contains(sql, "DELETE FROM skill_bundles"):
		if !tx.deleteHit {
			return bundleTestRow{err: pgx.ErrNoRows}
		}
		return bundleTestRow{values: []any{"abc", 2, int64(20)}}
	}
	tx.t.Fatalf("unexpected QueryRow in a bundle write: %s", sql)
	return bundleTestRow{err: pgx.ErrNoRows}
}

func (tx *bundleTestRecorder) Commit(context.Context) error {
	tx.committed = true
	return nil
}

func (tx *bundleTestRecorder) Rollback(context.Context) error { return nil }

type bundleTestRow struct {
	values []any
	err    error
}

func (row bundleTestRow) Scan(dest ...any) error {
	if row.err != nil {
		return row.err
	}
	for i, target := range dest {
		switch typed := target.(type) {
		case *string:
			*typed = row.values[i].(string)
		case *int:
			*typed = row.values[i].(int)
		case *int64:
			*typed = row.values[i].(int64)
		case *time.Time:
			*typed = row.values[i].(time.Time)
		}
	}
	return nil
}

// bundleTestDB hands out one recorder per write. A test that expects no write leaves tx nil,
// and any touch of the database then fails the test.
type bundleTestDB struct {
	t  *testing.T
	tx *bundleTestRecorder
}

func (d *bundleTestDB) Begin(context.Context) (pgx.Tx, error) {
	if d.tx == nil {
		d.t.Fatal("the registry was written for a request that had to be refused")
	}
	return d.tx, nil
}

func (d *bundleTestDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Fatal("bundle writes must run in a transaction")
	return pgconn.CommandTag{}, nil
}

func (d *bundleTestDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.t.Fatal("bundle reads are not part of this test")
	return nil, nil
}

func (d *bundleTestDB) QueryRow(context.Context, string, ...any) pgx.Row {
	d.t.Fatal("bundle reads are not part of this test")
	return nil
}

func bundleTestAdmin() authctx.Principal {
	return authctx.Principal{OrganizationID: "org-a", ActorID: "user-admin", PrincipalType: "user", Scopes: []string{authz.WriteScope, "org:admin"}, RetentionPolicyPresent: true}
}

// serveBundle answers one request on a mux that carries the bundle route, as protectedMux does.
func serveBundle(t *testing.T, enabled bool, db skillbundle.DB, principal *authctx.Principal, method, target string, body []byte) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewSkillBundlesHandler(db, enabled).Register(mux)
	request := httptest.NewRequest(method, target, bytes.NewReader(body))
	if principal != nil {
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), *principal))
	}
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

func TestSkillBundlesAPIIsOnlyTheLiteralTrue(t *testing.T) {
	for raw, want := range map[string]bool{"true": true, "1": false, "TRUE": false, " true": false, "yes": false, "": false} {
		got := SkillBundlesAPIFromLookup(func(string) string { return raw })
		if got != want {
			t.Errorf("SKILL_BUNDLES_ENABLED=%q enabled = %v, want %v", raw, got, want)
		}
	}
}

// While the switch is off every method is a 404 and the registry is not touched.
func TestSkillBundleRoutesAreNotFoundWhileTheAPIIsOff(t *testing.T) {
	admin := bundleTestAdmin()
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	for _, method := range []string{http.MethodGet, http.MethodPut, http.MethodDelete} {
		response := serveBundle(t, false, &bundleTestDB{t: t}, &admin, method, "/api/v1/skills/skill_1/bundle", upload)
		if response.Code != http.StatusNotFound {
			t.Fatalf("%s with the API off = %d, want 404", method, response.Code)
		}
	}
}

func TestSkillBundleWritesAreRefusedBeforeTheRegistryIsTouched(t *testing.T) {
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	noAuth := (*authctx.Principal)(nil)
	member := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", Scopes: []string{authz.WriteScope}, RetentionPolicyPresent: true}
	workload := bundleTestAdmin()
	workload.PrincipalType = "service"
	noWrite := bundleTestAdmin()
	noWrite.Scopes = []string{"org:admin"}

	tests := []struct {
		name      string
		principal *authctx.Principal
		method    string
		target    string
		status    int
		code      string
	}{
		{name: "no principal", principal: noAuth, method: http.MethodPut, target: "/api/v1/skills/skill_1/bundle", status: http.StatusUnauthorized, code: "authentication required"},
		{name: "a member is not an admin", principal: &member, method: http.MethodPut, target: "/api/v1/skills/skill_1/bundle", status: http.StatusForbidden, code: "only an admin may manage skill bundles"},
		{name: "a workload admin may not upload", principal: &workload, method: http.MethodPut, target: "/api/v1/skills/skill_1/bundle", status: http.StatusForbidden, code: "only a person may change a skill bundle"},
		{name: "an admin without write scope may not upload", principal: &noWrite, method: http.MethodPut, target: "/api/v1/skills/skill_1/bundle", status: http.StatusForbidden, code: "capability write scope required"},
		{name: "an admin without write scope may not delete", principal: &noWrite, method: http.MethodDelete, target: "/api/v1/skills/skill_1/bundle", status: http.StatusForbidden, code: "capability write scope required"},
		{name: "an invalid skill id is refused", principal: func() *authctx.Principal { p := bundleTestAdmin(); return &p }(), method: http.MethodPut, target: "/api/v1/skills/skill%21x/bundle", status: http.StatusBadRequest, code: "invalid skill id"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			response := serveBundle(t, true, &bundleTestDB{t: t}, tc.principal, tc.method, tc.target, upload)
			if response.Code != tc.status {
				t.Fatalf("status = %d, want %d (%s)", response.Code, tc.status, response.Body.String())
			}
			if got := orgAgentErrorCode(t, response); got != tc.code {
				t.Fatalf("error = %q, want %q", got, tc.code)
			}
		})
	}
}

func TestSkillBundleRefusesAnUnsafeUploadWithItsClosedCode(t *testing.T) {
	admin := bundleTestAdmin()
	response := serveBundle(t, true, &bundleTestDB{t: t}, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", []byte("not a zip"))
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422 (%s)", response.Code, response.Body.String())
	}
	if got := orgAgentErrorCode(t, response); got != "not_a_zip" {
		t.Fatalf("error = %q, want not_a_zip", got)
	}

	oversized := bytes.Repeat([]byte("a"), maxSkillBundleBodyBytes+1)
	response = serveBundle(t, true, &bundleTestDB{t: t}, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", oversized)
	if response.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("oversized status = %d, want 413", response.Code)
	}
}

func TestSkillBundleUploadStoresOnceAndAuditsWithoutContent(t *testing.T) {
	admin := bundleTestAdmin()
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD, "scripts/run.py": "print('secret body')"})
	tx := &bundleTestRecorder{t: t}
	response := serveBundle(t, true, &bundleTestDB{t: t, tx: tx}, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if response.Code != http.StatusCreated {
		t.Fatalf("first upload = %d, want 201 (%s)", response.Code, response.Body.String())
	}
	var view struct {
		SkillID   string `json:"skill_id"`
		FileCount int    `json:"file_count"`
		Replaced  bool   `json:"replaced"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &view); err != nil {
		t.Fatalf("response is not JSON: %v", err)
	}
	if view.SkillID != "skill_1" || view.FileCount != 2 || view.Replaced {
		t.Fatalf("view = %+v, want skill_1 with two files and no replacement", view)
	}
	if !tx.committed {
		t.Fatal("the bundle write did not commit")
	}
	audit := tx.execs[len(tx.execs)-1]
	if !strings.Contains(audit.sql, "INSERT INTO registry_audit_log") || !strings.Contains(audit.sql, "'skill_bundle'") {
		t.Fatalf("last statement is not the audit row: %s", audit.sql)
	}
	if !slicesContainsString(audit.args, "skill_bundle.uploaded") {
		t.Fatalf("audit args = %v, want the uploaded action", audit.args)
	}
	for _, arg := range audit.args {
		if text, ok := arg.(string); ok && strings.Contains(text, "secret body") {
			t.Fatal("the audit row carries a file body")
		}
	}
}

func TestSkillBundleReplacementAnswersOK(t *testing.T) {
	admin := bundleTestAdmin()
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})
	tx := &bundleTestRecorder{t: t, deleteRows: 1}
	response := serveBundle(t, true, &bundleTestDB{t: t, tx: tx}, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if response.Code != http.StatusOK {
		t.Fatalf("replacing upload = %d, want 200 (%s)", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"replaced":true`) {
		t.Fatalf("body = %s, want replaced true", response.Body.String())
	}
}

func TestSkillBundleDeleteRemovesOrNamesTheMissingBundle(t *testing.T) {
	admin := bundleTestAdmin()
	tx := &bundleTestRecorder{t: t, deleteHit: true}
	response := serveBundle(t, true, &bundleTestDB{t: t, tx: tx}, &admin, http.MethodDelete, "/api/v1/skills/skill_1/bundle", nil)
	if response.Code != http.StatusNoContent {
		t.Fatalf("delete = %d, want 204 (%s)", response.Code, response.Body.String())
	}
	if !tx.committed || !slicesContainsString(tx.execs[len(tx.execs)-1].args, "skill_bundle.deleted") {
		t.Fatalf("delete did not commit a deleted audit row: %+v", tx.execs)
	}

	missing := &bundleTestRecorder{t: t}
	response = serveBundle(t, true, &bundleTestDB{t: t, tx: missing}, &admin, http.MethodDelete, "/api/v1/skills/skill_1/bundle", nil)
	if response.Code != http.StatusNotFound {
		t.Fatalf("delete of a missing bundle = %d, want 404", response.Code)
	}
	if missing.committed {
		t.Fatal("a delete that found nothing committed a write")
	}
}

func TestSkillBundleRouteCoexistsWithTheSkillsSubtree(t *testing.T) {
	mux := http.NewServeMux()
	NewSkillsHandler(&recordingDatabase{}).Register(mux)
	NewSkillBundlesHandler(&bundleTestDB{t: t}, false).Register(mux)
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, httptest.NewRequest(http.MethodGet, "/api/v1/skills/skill_1/bundle", nil))
	if response.Code != http.StatusNotFound {
		t.Fatalf("bundle route with the API off = %d, want 404", response.Code)
	}
	response = httptest.NewRecorder()
	mux.ServeHTTP(response, httptest.NewRequest(http.MethodPost, "/api/v1/skills/skill_1/bundle", nil))
	if response.Code == http.StatusInternalServerError {
		t.Fatalf("skills subtree answered a bundle path with a server error")
	}
}

func slicesContainsString(values []any, want string) bool {
	for _, value := range values {
		if text, ok := value.(string); ok && text == want {
			return true
		}
	}
	return false
}
