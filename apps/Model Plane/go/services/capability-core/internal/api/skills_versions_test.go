package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// EP-11 S2: the skill history route and the version fields on skill reads.

// skillVersionRows answers the history query with the given versions, newest first.
type skillVersionRows struct {
	emptyRows
	versions []int32
	next     int
}

func (rows *skillVersionRows) Next() bool {
	rows.next++
	return rows.next <= len(rows.versions)
}

func (rows *skillVersionRows) Scan(dest ...any) error {
	if len(dest) != 7 {
		return errors.New("skillVersionRows: want seven scan destinations")
	}
	version := rows.versions[rows.next-1]
	*(dest[0].(*int32)) = version
	*(dest[1].(*string)) = strings.Repeat("a", 64)
	*(dest[2].(*skillVersionBody)) = skillVersionBody{Name: "deploy", Content: "step " + string(rune('0'+version))}
	*(dest[3].(*string)) = ""
	*(dest[4].(*string)) = "user"
	*(dest[5].(*string)) = "user-a"
	*(dest[6].(*time.Time)) = time.Unix(1_700_000_000, 0).UTC()
	return nil
}

func skillVersionsMux(t *testing.T, database *recordingDatabase, on bool) (http.Handler, mcpTokenSigner) {
	t.Helper()
	if on {
		t.Setenv(skillVersionsAPIEnv, "true")
	} else {
		t.Setenv(skillVersionsAPIEnv, "")
	}
	mux := http.NewServeMux()
	NewSkillsHandler(database).Register(mux)
	return mcpAuthenticatedHandlerWithSigner(t, mux)
}

func serveSkills(handler http.Handler, token, path string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodGet, path, nil)
	request.Header.Set("Authorization", "Bearer "+token)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

func TestSkillVersionsAPIReadsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "1": false, "TRUE": false, "on": false} {
		if got := SkillVersionsAPIFromLookup(func(string) string { return value }); got != want {
			t.Errorf("SKILL_VERSIONS_API=%q read as %v, want %v", value, got, want)
		}
	}
}

// Off, the history path is the skill subtree's GET as before: a skill id of "skill-1/versions"
// that is not found, and nothing reads the version table.
func TestSkillVersionsRouteIsAbsentWhileTheSwitchIsOff(t *testing.T) {
	database := &recordingDatabase{}
	handler, sign := skillVersionsMux(t, database, false)
	nonZDR := false
	response := serveSkills(handler, sign("user", nil, &nonZDR), "/api/v1/skills/skill-1/versions")
	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404", response.Code)
	}
	if len(database.queryRows) != 1 || database.queryRows[0].args[0] != "skill-1/versions" {
		t.Fatalf("calls = %#v, want only the plain skill read", database.queryRows)
	}
	if strings.Contains(database.queryRows[0].query, "version, digest") || len(database.queries) != 0 {
		t.Fatal("with the switch off no read names the version columns")
	}
}

func TestSkillVersionsRouteVisibility(t *testing.T) {
	for _, test := range []struct {
		name      string
		scopes    []string
		visible   bool
		wantAdmin bool
		wantCode  int
	}{
		{name: "owner", visible: true, wantCode: http.StatusOK},
		{name: "admin on a shared skill", scopes: []string{"admin"}, visible: true, wantAdmin: true, wantCode: http.StatusOK},
		{name: "stranger", visible: false, wantCode: http.StatusNotFound},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{rows: &skillVersionRows{versions: []int32{2, 1}}}
			database.nextRows = append(database.nextRows, boolRow{value: test.visible})
			handler, sign := skillVersionsMux(t, database, true)
			nonZDR := false
			response := serveSkills(handler, sign("user", test.scopes, &nonZDR), "/api/v1/skills/skill-1/versions")
			if response.Code != test.wantCode {
				t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), test.wantCode)
			}
			check := database.queryRows[0]
			if check.query != skillVisibleSQL {
				t.Fatalf("visibility query = %q", check.query)
			}
			if len(check.args) != 4 || check.args[0] != "skill-1" || check.args[1] != "org-a" ||
				check.args[2] != "user-a" || check.args[3] != test.wantAdmin {
				t.Fatalf("visibility args = %#v, want [skill-1 org-a user-a %v]", check.args, test.wantAdmin)
			}
			if !test.visible {
				var body struct {
					Error struct {
						Code    string         `json:"code"`
						Details map[string]any `json:"details"`
					} `json:"error"`
				}
				if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || body.Error.Code != "skill_not_found" || body.Error.Details == nil {
					t.Fatalf("refusal = %s, want the typed envelope with code skill_not_found", response.Body.String())
				}
				if len(database.queries) != 0 {
					t.Fatal("a skill the caller cannot see must not have its versions read")
				}
				return
			}
			var body struct {
				Versions []skillVersionView `json:"versions"`
				HasMore  bool               `json:"has_more"`
			}
			if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
				t.Fatal(err)
			}
			if len(body.Versions) != 2 || body.Versions[0].Version != 2 || body.Versions[1].Content != "step 1" || body.HasMore {
				t.Fatalf("versions = %+v, want v2 then v1 and no more", body)
			}
			if body.Versions[0].TriggerKeywords == nil {
				t.Fatal("an empty list must be a JSON array")
			}
			list := database.queries[0]
			if list.args[0] != "org-a" || list.args[1] != "skill-1" || list.args[2] != 0 || list.args[3] != defaultSkillVersionsPage+1 {
				t.Fatalf("history args = %#v", list.args)
			}
		})
	}
}

// The history route reads with exactly the rule get reads with, so it never shows a skill's text
// to someone get would refuse.
func TestSkillVersionsRouteUsesTheGetVisibility(t *testing.T) {
	database := &recordingDatabase{}
	handler, sign := skillVersionsMux(t, database, true)
	nonZDR := false
	serveSkills(handler, sign("user", nil, &nonZDR), "/api/v1/skills/skill-1")
	normalize := func(sql string) string { return strings.Join(strings.Fields(sql), " ") }
	get := normalize(database.queryRows[0].query)
	predicate := get[strings.Index(get, "WHERE id = $1"):]
	if !strings.Contains(normalize(skillVisibleSQL), predicate) {
		t.Fatalf("history visibility %q does not hold get's predicate %q", normalize(skillVisibleSQL), predicate)
	}
}

func TestSkillVersionsRouteBoundsItsPage(t *testing.T) {
	for path, want := range map[string]int{
		"/api/v1/skills/skill-1/versions?limit=0":           http.StatusBadRequest,
		"/api/v1/skills/skill-1/versions?limit=x":           http.StatusBadRequest,
		"/api/v1/skills/skill-1/versions?before_version=-1": http.StatusBadRequest,
		"/api/v1/skills/skill-1/versions?limit=500":         http.StatusOK,
	} {
		database := &recordingDatabase{rows: &skillVersionRows{}}
		database.nextRows = append(database.nextRows, boolRow{value: true})
		handler, sign := skillVersionsMux(t, database, true)
		nonZDR := false
		response := serveSkills(handler, sign("user", nil, &nonZDR), path)
		if response.Code != want {
			t.Errorf("%s: status = %d, want %d", path, response.Code, want)
		}
		if want == http.StatusOK && database.queries[0].args[3] != maxSkillVersionsPage+1 {
			t.Errorf("%s: limit = %v, want the cap", path, database.queries[0].args[3])
		}
	}
}

// On, list and get name version and digest; the writes never do (the trigger is their writer).
func TestSkillReadsCarryVersionOnlyWithTheSwitchOn(t *testing.T) {
	for _, on := range []bool{false, true} {
		database := &recordingDatabase{}
		handler, sign := skillVersionsMux(t, database, on)
		nonZDR := false
		serveSkills(handler, sign("user", nil, &nonZDR), "/api/v1/skills")
		serveSkills(handler, sign("user", nil, &nonZDR), "/api/v1/skills/skill-1")
		for _, query := range []string{database.queries[0].query, database.queryRows[0].query} {
			if got := strings.Contains(query, "updated_at, version, digest"); got != on {
				t.Errorf("switch %v: read names version and digest = %v\n%s", on, got, query)
			}
		}
	}
	var row skillRow
	if err := json.Unmarshal([]byte(`{"name":"x","content":"y"}`), &row); err != nil {
		t.Fatal(err)
	}
	encoded, _ := json.Marshal(row)
	if strings.Contains(string(encoded), `"version"`) || strings.Contains(string(encoded), `"digest"`) {
		t.Fatalf("a row read with the switch off carries no version fields: %s", encoded)
	}
}
