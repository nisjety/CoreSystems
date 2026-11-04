package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/services/capability-core/internal/skilllint"
)

// Templates and presets as authored kinds of agent_skills (EP-11 S6). The kind rules are tested
// against the shared golden in internal/skilllint; these tests prove the routes gate, filter and
// refuse as the design says, and that with AUTHORED_KINDS_ENABLED off nothing changes.

func setAuthoredKinds(t *testing.T, on bool) {
	t.Helper()
	if on {
		t.Setenv(authoredKindsEnv, "true")
	} else {
		t.Setenv(authoredKindsEnv, "")
	}
}

func typedErrorCode(t *testing.T, body []byte) string {
	t.Helper()
	var envelope struct {
		Error struct {
			Code    string         `json:"code"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil || envelope.Error.Details == nil {
		t.Fatalf("not the typed envelope with object details: %v (%s)", err, body)
	}
	return envelope.Error.Code
}

// storedKindRow answers the PATCH kind rules' read of the row they are about to change.
type storedKindRow struct {
	kind, content, settings string
}

func (row storedKindRow) Scan(dest ...any) error {
	if len(dest) != 3 {
		return errors.New("storedKindRow: the kind rules read kind, content and settings")
	}
	*dest[0].(*string), *dest[1].(*string) = row.kind, row.content
	*dest[2].(*json.RawMessage) = json.RawMessage(row.settings)
	return nil
}

func TestAuthoredKindsReadsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "1": false, "TRUE": false, "on": false} {
		if got := AuthoredKindsFromLookup(func(string) string { return value }); got != want {
			t.Errorf("AUTHORED_KINDS_ENABLED=%q read as %v, want %v", value, got, want)
		}
	}
}

func TestKindFilterDefaultsToSkill(t *testing.T) {
	setAuthoredKinds(t, true)
	for path, want := range map[string]string{
		"/api/v1/skills":               skilllint.KindSkill,
		"/api/v1/skills?kind=template": skilllint.KindTemplate,
		"/api/v1/skills?kind=preset":   skilllint.KindPreset,
	} {
		database := &recordingDatabase{}
		response := serveSkill(t, database, writeToken, http.MethodGet, path, "")
		if response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"kinds_enabled":true`) {
			t.Fatalf("%s: %d %s", path, response.Code, response.Body.String())
		}
		query := database.queries[0]
		if !strings.Contains(query.query, "AND kind = $4") || len(query.args) != 4 || query.args[3] != want {
			t.Errorf("%s: query %q args %v, want kind %s", path, query.query, query.args, want)
		}
		if !strings.Contains(query.query, ", kind, settings") {
			t.Errorf("%s: the read does not return kind and settings", path)
		}
	}
	bad := serveSkill(t, &recordingDatabase{}, writeToken, http.MethodGet, "/api/v1/skills?kind=agent", "")
	if bad.Code != http.StatusUnprocessableEntity || typedErrorCode(t, bad.Body.Bytes()) != "invalid_kind" {
		t.Fatalf("an unknown kind = %d %s, want 422 invalid_kind", bad.Code, bad.Body.String())
	}
}

// Off, the skills read is the statement and body it always was, and any other kind answers an
// empty list that says why, without touching the registry.
func TestKindsOffKeepsTheSkillsReadAndSaysWhy(t *testing.T) {
	setAuthoredKinds(t, false)
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodGet, "/api/v1/skills", "")
	if response.Code != http.StatusOK || strings.Contains(response.Body.String(), "kinds_enabled") {
		t.Fatalf("off list = %d %s, want the old body", response.Code, response.Body.String())
	}
	if query := database.queries[0]; strings.Contains(query.query, "kind") || len(query.args) != 3 {
		t.Fatalf("off list changed its statement: %q %v", query.query, query.args)
	}
	templates := &recordingDatabase{}
	response = serveSkill(t, templates, writeToken, http.MethodGet, "/api/v1/skills?kind=template", "")
	if response.Code != http.StatusOK || strings.TrimSpace(response.Body.String()) != `{"kinds_enabled":false,"skills":[]}` {
		t.Fatalf("off template list = %d %s", response.Code, response.Body.String())
	}
	if templates.databaseCalls() != 0 {
		t.Fatal("an off template list read the registry")
	}
}

func TestPostKindNotEnabled(t *testing.T) {
	setAuthoredKinds(t, false)
	for _, body := range []string{
		`{"name":"Weekly","scope":"user","content":"Report {{input}}","kind":"template"}`,
		`{"name":"Weekly","scope":"user","content":"c","settings":{"browseWeb":true}}`,
	} {
		database := &recordingDatabase{}
		response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills", body)
		if response.Code != http.StatusUnprocessableEntity || typedErrorCode(t, response.Body.Bytes()) != "kind_not_enabled" {
			t.Fatalf("%s: %d %s, want 422 kind_not_enabled", body, response.Code, response.Body.String())
		}
		if database.databaseCalls() != 0 || database.commits != 0 {
			t.Fatalf("%s: a refused create touched the registry", body)
		}
	}
	// A body that names the skill kind is the create it always was.
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills", `{"name":"Weekly","scope":"user","content":"c","kind":"skill"}`)
	if response.Code != http.StatusCreated || strings.Contains(database.execs[0].query, "kind") {
		t.Fatalf("off skill create = %d %s %q", response.Code, response.Body.String(), database.execs[0].query)
	}
}

// The preset rules run whatever SKILL_LINT_MODE says: a preset with a key outside the closed
// schema is refused even with the lint off.
func TestPresetSettingsClosedSchema(t *testing.T) {
	setAuthoredKinds(t, true)
	t.Setenv(skilllint.ModeEnv, "")
	database := &recordingDatabase{}
	body := `{"name":"Pirate","scope":"user","content":"c","kind":"preset","settings":{"persona":"pirate","toolAllowList":["*"]}}`
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills", body)
	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want 422 (%s)", response.Code, response.Body.String())
	}
	if got := lintCodes(decodeLintRefusal(t, response.Body.Bytes()).Error.Details.Findings); strings.Join(got, ",") != skilllint.CodePresetUnknownKey {
		t.Fatalf("findings = %v", got)
	}
	if database.databaseCalls() != 0 {
		t.Fatal("a refused preset touched the registry")
	}
}

func TestPresetPlanModeMustBeTrue(t *testing.T) {
	setAuthoredKinds(t, true)
	t.Setenv(skilllint.ModeEnv, "")
	refused := serveSkill(t, &recordingDatabase{}, writeToken, http.MethodPost, "/api/v1/skills",
		`{"name":"Fast","scope":"user","content":"c","kind":"preset","settings":{"planMode":false}}`)
	if refused.Code != http.StatusUnprocessableEntity {
		t.Fatalf("planMode false = %d, want 422 (%s)", refused.Code, refused.Body.String())
	}
	if got := lintCodes(decodeLintRefusal(t, refused.Body.Bytes()).Error.Details.Findings); strings.Join(got, ",") != skilllint.CodePresetSettingsInvalid {
		t.Fatalf("findings = %v", got)
	}

	database := &recordingDatabase{}
	created := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills",
		`{"name":"Research","scope":"user","content":"Research {{input}}","kind":"preset","settings":{"modeId":"research","browseWeb":true,"planMode":true}}`)
	if created.Code != http.StatusCreated {
		t.Fatalf("a valid preset = %d %s", created.Code, created.Body.String())
	}
	insert := database.execs[0]
	if !strings.Contains(insert.query, "kind, settings)") {
		t.Fatalf("the create does not write kind and settings: %q", insert.query)
	}
	if insert.args[14] != skilllint.KindPreset || !strings.Contains(string(insert.args[15].([]byte)), `"planMode":true`) {
		t.Fatalf("kind and settings args = %v %s", insert.args[14], insert.args[15])
	}
}

func TestTemplatePlaceholderAndSettingsRules(t *testing.T) {
	setAuthoredKinds(t, true)
	t.Setenv(skilllint.ModeEnv, "")
	placeholder := serveSkill(t, &recordingDatabase{}, writeToken, http.MethodPost, "/api/v1/skills",
		`{"name":"Weekly","scope":"user","content":"Dear {{name}}","kind":"template"}`)
	if got := lintCodes(decodeLintRefusal(t, placeholder.Body.Bytes()).Error.Details.Findings); placeholder.Code != http.StatusUnprocessableEntity || strings.Join(got, ",") != skilllint.CodeTemplatePlaceholder {
		t.Fatalf("placeholder = %d %v", placeholder.Code, got)
	}
	settings := serveSkill(t, &recordingDatabase{}, writeToken, http.MethodPost, "/api/v1/skills",
		`{"name":"Weekly","scope":"user","content":"Report {{input}}","kind":"template","settings":{"browseWeb":true}}`)
	if settings.Code != http.StatusUnprocessableEntity || typedErrorCode(t, settings.Body.Bytes()) != "settings_not_allowed" {
		t.Fatalf("template settings = %d %s, want 422 settings_not_allowed", settings.Code, settings.Body.String())
	}
	database := &recordingDatabase{}
	created := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills",
		`{"name":"Weekly","scope":"user","content":"Report for {{input}}","kind":"template"}`)
	if created.Code != http.StatusCreated || string(database.execs[0].args[15].([]byte)) != "{}" {
		t.Fatalf("a valid template = %d %s", created.Code, created.Body.String())
	}
}

func TestPatchSettingsOnlyOnPresets(t *testing.T) {
	setAuthoredKinds(t, true)
	t.Setenv(skilllint.ModeEnv, "")
	template := &recordingDatabase{nextRows: []pgx.Row{storedKindRow{kind: "template", content: "Report {{input}}", settings: "{}"}}}
	response := serveSkill(t, template, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"settings":{"browseWeb":true}}`)
	if response.Code != http.StatusUnprocessableEntity || typedErrorCode(t, response.Body.Bytes()) != "settings_not_allowed" {
		t.Fatalf("template settings patch = %d %s", response.Code, response.Body.String())
	}
	if len(template.execs) != 0 || template.commits != 0 {
		t.Fatal("a refused patch ran its UPDATE")
	}

	preset := &recordingDatabase{nextRows: []pgx.Row{storedKindRow{kind: "preset", content: "c", settings: `{"planMode":true}`}}}
	response = serveSkill(t, preset, writeToken, http.MethodPatch, "/api/v1/skills/sk-2", `{"settings":{"planMode":false}}`)
	if got := lintCodes(decodeLintRefusal(t, response.Body.Bytes()).Error.Details.Findings); response.Code != http.StatusUnprocessableEntity || strings.Join(got, ",") != skilllint.CodePresetSettingsInvalid {
		t.Fatalf("preset planMode patch = %d %v", response.Code, got)
	}

	valid := &recordingDatabase{nextRows: []pgx.Row{storedKindRow{kind: "preset", content: "c", settings: "{}"}}}
	response = serveSkill(t, valid, writeToken, http.MethodPatch, "/api/v1/skills/sk-3", `{"settings":{"browseWeb":true}}`)
	if response.Code != http.StatusOK || !strings.Contains(valid.execs[0].query, "settings=COALESCE($8::jsonb, settings)") {
		t.Fatalf("a valid preset patch = %d %s", response.Code, response.Body.String())
	}
}

// Off, a PATCH that sends settings is the PATCH it always was: settings are ignored and the
// statement does not name them.
func TestPatchIgnoresSettingsWhileKindsAreOff(t *testing.T) {
	setAuthoredKinds(t, false)
	t.Setenv(skilllint.ModeEnv, "")
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"content":"c","settings":{"planMode":false}}`)
	if response.Code != http.StatusOK || strings.Contains(database.execs[0].query, "settings") || len(database.queryRows) != 0 {
		t.Fatalf("off patch = %d %s %q", response.Code, response.Body.String(), database.execs[0].query)
	}
}
