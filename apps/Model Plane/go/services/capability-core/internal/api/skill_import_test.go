package api

import (
	"archive/zip"
	"bytes"
	"encoding/base64"
	"encoding/json"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/skilllint"
)

// EP-11 S8: importing a skill from a SKILL.md or a zip creates it switched off, as a draft.

const importSkillMD = "---\nname: Utrulling\ndescription: Steps for a release\n---\n\n1. Run the tests\n2. Deploy\n"

type importZipEntry struct {
	name string
	body string
	mode fs.FileMode
}

func importZip(t *testing.T, entries ...importZipEntry) []byte {
	t.Helper()
	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)
	for _, entry := range entries {
		header := &zip.FileHeader{Name: entry.name, Method: zip.Deflate}
		mode := entry.mode
		if mode == 0 {
			mode = 0o644
		}
		header.SetMode(mode)
		file, err := writer.CreateHeader(header)
		if err != nil {
			t.Fatalf("create %s: %v", entry.name, err)
		}
		if _, err := file.Write([]byte(entry.body)); err != nil {
			t.Fatalf("write %s: %v", entry.name, err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatalf("close zip: %v", err)
	}
	return buffer.Bytes()
}

func importHandler(database *recordingDatabase, retention bool) *SkillImportHandler {
	return &SkillImportHandler{
		skills:  &SkillsHandler{pool: database, lintMode: skilllint.ModeOff, bundleRetention: retention},
		enabled: true,
	}
}

func importPrincipal(admin bool) authctx.Principal {
	scopes := []string{authz.WriteScope}
	if admin {
		scopes = append(scopes, "admin")
	}
	return authctx.Principal{
		OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", Scopes: scopes,
		RetentionPolicyPresent: true,
	}
}

func importBody(t *testing.T, file []byte, scope string) string {
	t.Helper()
	body, err := json.Marshal(map[string]string{"content_base64": base64.StdEncoding.EncodeToString(file), "scope": scope})
	if err != nil {
		t.Fatal(err)
	}
	return string(body)
}

func postImport(t *testing.T, handler *SkillImportHandler, principal authctx.Principal, body string) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	handler.Register(mux)
	request := httptest.NewRequest(http.MethodPost, "/api/v1/skills/import", strings.NewReader(body))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

// execWith returns the first recorded statement holding fragment.
func execWith(database *recordingDatabase, fragment string) (databaseCall, bool) {
	for _, call := range database.execs {
		if strings.Contains(call.query, fragment) {
			return call, true
		}
	}
	return databaseCall{}, false
}

func TestSkillImportReadsOnlyTheLiteralTrueWithBundlesAndDrafts(t *testing.T) {
	all := map[string]string{skillImportEnv: "true", skillBundlesAPIEnv: "true", draftPublishEnv: "true"}
	if !SkillImportFromLookup(func(key string) string { return all[key] }) {
		t.Fatal("all three switches on must mount the import")
	}
	for _, value := range []string{"", "1", "TRUE", "on"} {
		env := map[string]string{skillImportEnv: value, skillBundlesAPIEnv: "true", draftPublishEnv: "true"}
		if SkillImportFromLookup(func(key string) string { return env[key] }) {
			t.Errorf("SKILL_IMPORT_ENABLED=%q mounted the import", value)
		}
	}
	for _, missing := range []string{skillBundlesAPIEnv, draftPublishEnv} {
		env := map[string]string{skillImportEnv: "true", skillBundlesAPIEnv: "true", draftPublishEnv: "true"}
		delete(env, missing)
		if SkillImportFromLookup(func(key string) string { return env[key] }) {
			t.Errorf("the import mounted without %s", missing)
		}
	}
}

// Off, the route is not mounted and nothing is read or written.
func TestImportRouteIsAbsentWhileTheSwitchIsOff(t *testing.T) {
	database := &recordingDatabase{}
	handler := importHandler(database, false)
	handler.enabled = false
	response := postImport(t, handler, importPrincipal(false), importBody(t, []byte(importSkillMD), ""))
	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", response.Code)
	}
	if database.databaseCalls() != 0 || database.commits != 0 {
		t.Fatal("the registry was touched with the switch off")
	}
}

func TestImportSkillMdCreatesDraft(t *testing.T) {
	database := &recordingDatabase{}
	response := postImport(t, importHandler(database, false), importPrincipal(false), importBody(t, []byte(importSkillMD), ""))
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.execs) != 2 || database.execs[0].query != importSkillSQL || database.execs[1].query != importDraftSQL {
		t.Fatalf("statements = %#v, want the insert then the draft", database.execs)
	}
	args := database.execs[0].args
	if args[2] != "Utrulling" || args[3] != "Steps for a release" || args[4] != "1. Run the tests\n2. Deploy" ||
		args[5] != "user" || args[6] != "user-a" {
		t.Fatalf("insert args = %#v", args)
	}
	if !strings.Contains(importSkillSQL, "false, $6") {
		t.Fatal("an imported skill must be created switched off")
	}
	if draft := database.execs[1].args; draft[2] != "" || draft[4] != "user-a" {
		t.Fatalf("draft args = %#v, want no bundle and the importer", draft)
	}
	if len(database.actorCalls) != 1 || database.actorCalls[0].args[1] != "user-a" || database.actorCalls[0].args[2] != "imported" {
		t.Fatalf("actor = %#v, want the verified person and the import reason", database.actorCalls)
	}
	if database.commits != 1 {
		t.Fatalf("commits = %d, want one transaction", database.commits)
	}
	var body map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body["draft"] != true || body["enabled"] != false || body["scope"] != "user" || body["bundle"] != nil {
		t.Fatalf("body = %s", response.Body.String())
	}
}

func TestImportZipCreatesDraftBundle(t *testing.T) {
	database := &recordingDatabase{}
	file := importZip(t,
		importZipEntry{name: "SKILL.md", body: importSkillMD},
		importZipEntry{name: "scripts/run.py", body: "print(1)\n", mode: 0o755},
	)
	response := postImport(t, importHandler(database, true), importPrincipal(true), importBody(t, file, "user"))
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	bundle, ok := execWith(database, "INSERT INTO skill_bundles")
	if !ok || !strings.Contains(bundle.query, "'draft'") {
		t.Fatalf("the bundle must be stored as a draft: %#v", database.execs)
	}
	manifest, _ := bundle.args[3].(string)
	if manifest == "" || bundle.args[2] != database.execs[0].args[0] {
		t.Fatalf("bundle args = %#v, want the new skill and its manifest", bundle.args)
	}
	if audit, ok := execWith(database, "INSERT INTO registry_audit_log"); !ok || audit.args[2] != "skill_bundle.imported" {
		t.Fatalf("audit = %#v", audit)
	}
	draft, ok := execWith(database, "SET draft = agent_skill_version_body")
	if !ok || draft.args[2] != manifest {
		t.Fatalf("the draft must name the draft bundle: %#v", draft)
	}
	if database.commits != 1 {
		t.Fatalf("commits = %d, want the skill and its files in one transaction", database.commits)
	}
	var body struct {
		Bundle struct {
			FileCount int `json:"file_count"`
		} `json:"bundle"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || body.Bundle.FileCount != 2 {
		t.Fatalf("body = %s", response.Body.String())
	}
}

// A zip with nothing beside SKILL.md is the file alone: any member may import it, with no bundle.
func TestImportZipWithOnlySkillMdStoresNoBundle(t *testing.T) {
	database := &recordingDatabase{}
	file := importZip(t, importZipEntry{name: "SKILL.md", body: importSkillMD})
	response := postImport(t, importHandler(database, false), importPrincipal(false), importBody(t, file, ""))
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if _, ok := execWith(database, "skill_bundles"); ok {
		t.Fatalf("no bundle may be stored: %#v", database.execs)
	}
}

// Files beside SKILL.md are admin-only, and need retention so a publish can make them live.
func TestImportBundleNeedsAdminAndRetention(t *testing.T) {
	file := importZip(t,
		importZipEntry{name: "SKILL.md", body: importSkillMD},
		importZipEntry{name: "references/notes.md", body: "notes"},
	)
	for _, test := range []struct {
		name      string
		admin     bool
		retention bool
		status    int
		code      string
	}{
		{"member", false, true, http.StatusForbidden, "skill_import_bundle_requires_admin"},
		{"admin without retention", true, false, http.StatusConflict, "skill_import_bundle_unavailable"},
	} {
		database := &recordingDatabase{}
		response := postImport(t, importHandler(database, test.retention), importPrincipal(test.admin), importBody(t, file, ""))
		if response.Code != test.status || errorCode(t, response) != test.code {
			t.Errorf("%s: status = %d, body=%s", test.name, response.Code, response.Body.String())
		}
		if database.databaseCalls() != 0 {
			t.Errorf("%s: the registry was touched", test.name)
		}
	}
}

func TestImportRefusesZipSlipAndSymlink(t *testing.T) {
	for _, test := range []struct {
		name  string
		entry importZipEntry
		code  string
	}{
		{"zip slip", importZipEntry{name: "../evil.sh", body: "rm -rf /"}, "unsafe_path"},
		{"symlink", importZipEntry{name: "secrets", body: "/etc/passwd", mode: fs.ModeSymlink | 0o777}, "symlink_refused"},
		{"exec bit outside scripts", importZipEntry{name: "run.sh", body: "echo", mode: 0o755}, "exec_bit_outside_scripts"},
	} {
		database := &recordingDatabase{}
		file := importZip(t, importZipEntry{name: "SKILL.md", body: importSkillMD}, test.entry)
		response := postImport(t, importHandler(database, true), importPrincipal(true), importBody(t, file, ""))
		if response.Code != http.StatusUnprocessableEntity || errorCode(t, response) != test.code {
			t.Errorf("%s: status = %d, body=%s", test.name, response.Code, response.Body.String())
		}
		if database.databaseCalls() != 0 {
			t.Errorf("%s: the registry was touched", test.name)
		}
	}
}

func TestImportRefusesASkillMdWithoutFrontMatter(t *testing.T) {
	database := &recordingDatabase{}
	response := postImport(t, importHandler(database, false), importPrincipal(false), importBody(t, []byte("# No front matter\n"), ""))
	if response.Code != http.StatusUnprocessableEntity || errorCode(t, response) != "skill_md_front_matter" {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatal("the registry was touched")
	}
}

func TestImportOrgScopeAdminOnly(t *testing.T) {
	database := &recordingDatabase{}
	response := postImport(t, importHandler(database, false), importPrincipal(false), importBody(t, []byte(importSkillMD), "org"))
	if response.Code != http.StatusForbidden || database.databaseCalls() != 0 {
		t.Fatalf("a member's org import: status = %d, calls = %d", response.Code, database.databaseCalls())
	}
	database = &recordingDatabase{}
	response = postImport(t, importHandler(database, false), importPrincipal(true), importBody(t, []byte(importSkillMD), "org"))
	if response.Code != http.StatusCreated {
		t.Fatalf("an admin's org import: status = %d, body=%s", response.Code, response.Body.String())
	}
	if args := database.execs[0].args; args[5] != "org" || args[6] != "" {
		t.Fatalf("an org skill has no owner: %#v", args)
	}
}

// The lint runs on every import, whatever SKILL_LINT_MODE says: an over-long body is refused
// before anything is written.
func TestImportLintsBody(t *testing.T) {
	database := &recordingDatabase{}
	long := "---\nname: Lang\ndescription: Too long\n---\n" + strings.Repeat("a", skilllint.MaxContentChars+1)
	response := postImport(t, importHandler(database, false), importPrincipal(false), importBody(t, []byte(long), ""))
	if response.Code != http.StatusUnprocessableEntity || errorCode(t, response) != skilllint.ErrorCode {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if !strings.Contains(response.Body.String(), skilllint.CodeContentTooLong) {
		t.Fatalf("body = %s, want the content rule named", response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatal("a refused import touched the registry")
	}
}

func TestImportNameTakenIsAConflict(t *testing.T) {
	database := &recordingDatabase{execErr: &pgconn.PgError{Code: "23505"}, execErrAt: 1}
	response := postImport(t, importHandler(database, false), importPrincipal(false), importBody(t, []byte(importSkillMD), ""))
	if response.Code != http.StatusConflict || errorCode(t, response) != "skill_name_taken" {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if database.commits != 0 {
		t.Fatal("a refused import committed")
	}
}

// A ZDR principal is refused with the catalogue's refusal before anything is read, and through the
// real middleware a ZDR token never reaches the handler at all.
func TestImportRefusesZDRPrincipals(t *testing.T) {
	database := &recordingDatabase{}
	principal := importPrincipal(true)
	principal.ZeroDataRetention = true
	response := postImport(t, importHandler(database, true), principal, importBody(t, []byte(importSkillMD), ""))
	if response.Code != http.StatusPreconditionFailed || errorCode(t, response) != "zdr_feature_not_supported" ||
		!strings.Contains(response.Body.String(), `"feature":"skill_authoring"`) {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatal("a ZDR import touched the registry")
	}

	database = &recordingDatabase{}
	mux := http.NewServeMux()
	importHandler(database, true).Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	zdr := true
	request := httptest.NewRequest(http.MethodPost, "/api/v1/skills/import", strings.NewReader(importBody(t, []byte(importSkillMD), "")))
	request.Header.Set("Authorization", "Bearer "+sign("user", []string{authz.WriteScope, "admin"}, &zdr))
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, request)
	if recorder.Code == http.StatusCreated || database.databaseCalls() != 0 {
		t.Fatalf("a ZDR token imported: status = %d, calls = %d", recorder.Code, database.databaseCalls())
	}
}

func TestImportRequiresAPerson(t *testing.T) {
	database := &recordingDatabase{}
	principal := importPrincipal(true)
	principal.PrincipalType = "service"
	response := postImport(t, importHandler(database, true), principal, importBody(t, []byte(importSkillMD), ""))
	if response.Code != http.StatusForbidden || errorCode(t, response) != "skill_import_requires_person" {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatal("a service import touched the registry")
	}
}
