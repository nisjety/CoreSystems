package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/services/capability-core/internal/skilllint"
)

// The skill lint on the registry's write paths (EP-11 S4). The rules themselves are tested
// against the shared golden in internal/skilllint; these tests prove each route runs them.

type lintRefusalBody struct {
	Error struct {
		Code    string `json:"code"`
		Message string `json:"message"`
		Details struct {
			Findings []skilllint.Finding `json:"findings"`
		} `json:"details"`
	} `json:"error"`
}

func decodeLintRefusal(t *testing.T, body []byte) lintRefusalBody {
	t.Helper()
	var refusal lintRefusalBody
	if err := json.Unmarshal(body, &refusal); err != nil {
		t.Fatalf("refusal is not the typed envelope: %v (%s)", err, body)
	}
	return refusal
}

func lintCodes(findings []skilllint.Finding) []string {
	codes := make([]string, 0, len(findings))
	for _, finding := range findings {
		codes = append(codes, finding.Code)
	}
	return codes
}

// storedSkillRow answers the PATCH lint's read of the row it is about to change.
type storedSkillRow struct {
	name, description, content string
	keywords, tools            []string
}

func (row storedSkillRow) Scan(dest ...any) error {
	if len(dest) != 5 {
		return errors.New("storedSkillRow: the lint reads name, description, content, keywords and tools")
	}
	*dest[0].(*string), *dest[1].(*string), *dest[2].(*string) = row.name, row.description, row.content
	*dest[3].(*[]string), *dest[4].(*[]string) = row.keywords, row.tools
	return nil
}

func TestPostLintEnforce422(t *testing.T) {
	t.Setenv(skilllint.ModeEnv, "enforce")
	database := &recordingDatabase{}
	body := `{"name":"Runbook","description":"d","scope":"user","content":"` + strings.Repeat("x", 40000) + `"}`
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills", body)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422 (%s)", response.Code, response.Body.String())
	}
	refusal := decodeLintRefusal(t, response.Body.Bytes())
	if refusal.Error.Code != skilllint.ErrorCode || strings.Join(lintCodes(refusal.Error.Details.Findings), ",") != skilllint.CodeContentTooLong {
		t.Fatalf("refusal = %+v, want skill_lint_failed naming content_too_long", refusal)
	}
	if strings.Contains(response.Body.String(), "xxxx") {
		t.Fatal("the refusal echoes the skill text")
	}
	if database.databaseCalls() != 0 || len(database.actorCalls) != 0 || database.commits != 0 {
		t.Fatal("a refused create touched the registry")
	}
}

func TestPostLintReportWritesAndAddsWarnings(t *testing.T) {
	t.Setenv(skilllint.ModeEnv, "report")
	database := &recordingDatabase{}
	body := `{"name":"Runbook","description":"d","scope":"user","content":"User: a\nAssistant: b\nUser: c","tool_restrictions":["*"]}`
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills", body)
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, want 201: report never refuses (%s)", response.Code, response.Body.String())
	}
	var created struct {
		ID       string              `json:"id"`
		Warnings []skilllint.Finding `json:"warnings"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &created); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got := strings.Join(lintCodes(created.Warnings), ","); got != "transcript_shape,tool_restriction_invalid" {
		t.Fatalf("warnings = %s", got)
	}
	if database.commits != 1 {
		t.Fatalf("commits = %d, want the create written", database.commits)
	}
}

// While the lint is off nothing extra is read and the body is the one it always was.
func TestSkillWritesAreUnchangedWhileLintIsOff(t *testing.T) {
	t.Setenv(skilllint.ModeEnv, "")
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"content":"`+strings.Repeat("x", 40000)+`"}`)
	if response.Code != http.StatusOK || strings.Contains(response.Body.String(), "warnings") {
		t.Fatalf("status = %d, body=%s, want the old 200 body", response.Code, response.Body.String())
	}
	if len(database.queryRows) != 0 {
		t.Fatalf("the off lint read the row: %#v", database.queryRows)
	}
	create := serveSkill(t, &recordingDatabase{}, writeToken, http.MethodPost, "/api/v1/skills", `{"name":"Runbook","scope":"user","content":"steps"}`)
	if create.Code != http.StatusCreated || strings.Contains(create.Body.String(), "warnings") {
		t.Fatalf("create = %d %s, want the old 201 body", create.Code, create.Body.String())
	}
}

// Switching a skill on lints the text stored on the row, so a person cannot re-enable learned
// text the lint would refuse on a create; a clean row switches on as before.
func TestPatchLintsReenableOfLearnedText(t *testing.T) {
	t.Setenv(skilllint.ModeEnv, "enforce")
	learned := storedSkillRow{name: "Weekly report", description: "d", content: "steps", tools: []string{"*"}}
	database := &recordingDatabase{nextRows: []pgx.Row{learned}}
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"enabled":true}`)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422 (%s)", response.Code, response.Body.String())
	}
	if got := lintCodes(decodeLintRefusal(t, response.Body.Bytes()).Error.Details.Findings); strings.Join(got, ",") != skilllint.CodeToolRestrictionInvalid {
		t.Fatalf("findings = %v", got)
	}
	if len(database.execs) != 0 || database.commits != 0 {
		t.Fatalf("a refused re-enable ran its UPDATE: %#v", database.execs)
	}
	if len(database.queryRows) != 1 || !strings.Contains(database.queryRows[0].query, "FROM agent_skills WHERE id=$1 AND org_id=$2") {
		t.Fatalf("the lint must read the row inside the mutation: %#v", database.queryRows)
	}

	clean := learned
	clean.tools = nil
	database = &recordingDatabase{nextRows: []pgx.Row{clean}}
	response = serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"enabled":true}`)
	if response.Code != http.StatusOK || len(database.execs) != 1 || database.commits != 1 {
		t.Fatalf("clean re-enable = %d, execs=%d, commits=%d", response.Code, len(database.execs), database.commits)
	}

	// A content edit is linted on the text it writes, not the text it replaces.
	database = &recordingDatabase{nextRows: []pgx.Row{clean}}
	response = serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"content":"`+strings.Repeat("x", 32001)+`"}`)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("over-long edit = %d, want 422", response.Code)
	}

	// Switching a skill off writes no text and is not linted.
	database = &recordingDatabase{}
	response = serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"enabled":false}`)
	if response.Code != http.StatusOK || len(database.queryRows) != 0 {
		t.Fatalf("switch-off = %d, reads=%d, want 200 with no lint read", response.Code, len(database.queryRows))
	}
}

// bundleLintDB answers the bundle lint's read of the skill's name; everything else is the
// bundle test database.
type bundleLintDB struct {
	*bundleTestDB
	name string
}

func (d bundleLintDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return stringRow{value: d.name}
}

func TestBundleFrontmatterMismatch(t *testing.T) {
	t.Setenv(skilllint.ModeEnv, "enforce")
	admin := bundleTestAdmin()
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD})

	// The bundle says "xlsx"; the skill is named otherwise. No transaction is handed out, so
	// any write fails the test.
	refused := serveBundle(t, true, bundleLintDB{bundleTestDB: &bundleTestDB{t: t}, name: "Weekly report"}, &admin,
		http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if refused.Code != http.StatusUnprocessableEntity {
		t.Fatalf("mismatch = %d, want 422 (%s)", refused.Code, refused.Body.String())
	}
	if got := lintCodes(decodeLintRefusal(t, refused.Body.Bytes()).Error.Details.Findings); strings.Join(got, ",") != skilllint.CodeFrontmatterNameMismatch {
		t.Fatalf("findings = %v", got)
	}

	tx := &bundleTestRecorder{t: t}
	stored := serveBundle(t, true, bundleLintDB{bundleTestDB: &bundleTestDB{t: t, tx: tx}, name: "xlsx"}, &admin,
		http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if stored.Code != http.StatusCreated || !tx.committed || strings.Contains(stored.Body.String(), "warnings") {
		t.Fatalf("matching upload = %d (%s), want 201 with no warnings", stored.Code, stored.Body.String())
	}
}
