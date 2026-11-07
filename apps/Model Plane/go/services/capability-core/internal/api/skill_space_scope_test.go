package api

import (
	"encoding/json"
	"net/http"
	"testing"
)

func TestSkillSpaceScopeReadsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "1": false, "TRUE": false, "on": false} {
		if got := SkillSpaceScopeFromLookup(func(string) string { return value }); got != want {
			t.Errorf("SKILL_SPACE_SCOPE=%q read as %v, want %v", value, got, want)
		}
	}
}

// requireSpaceDecisionRefusal checks the typed envelope of a refused Space skill write.
func requireSpaceDecisionRefusal(t *testing.T, code int, body []byte) {
	t.Helper()
	if code != http.StatusForbidden {
		t.Fatalf("status = %d, body=%s, want 403", code, body)
	}
	var envelope struct {
		Error struct {
			Code    string         `json:"code"`
			Message string         `json:"message"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		t.Fatalf("body is not the typed envelope: %v (%s)", err, body)
	}
	if envelope.Error.Code != spaceSkillDecisionRequiredCode || envelope.Error.Details == nil {
		t.Fatalf("refusal = %+v, want code %q with object details", envelope.Error, spaceSkillDecisionRequiredCode)
	}
}

// EP-11 S9 (D7): no Space skill is written on an org role. With the scope on, a create of one is
// refused before the database, even for an admin; a change or delete of one is refused under
// the row lock, before its statement, even for an admin. Off, "space" is an invalid scope as before.
func TestSpaceSkillWriteNeedsDecision(t *testing.T) {
	t.Run("create, switch off", func(t *testing.T) {
		t.Setenv(skillSpaceScopeEnv, "")
		database := &recordingDatabase{}
		response := serveSkill(t, database, adminToken, http.MethodPost, "/api/v1/skills",
			`{"name":"room skill","content":"body","scope":"space"}`)
		if response.Code != http.StatusUnprocessableEntity {
			t.Fatalf("status = %d, body=%s, want 422 (invalid scope, as before)", response.Code, response.Body.String())
		}
		if database.databaseCalls() != 0 {
			t.Fatalf("a refused create must never reach the database, got %d calls", database.databaseCalls())
		}
	})
	t.Run("create, switch on", func(t *testing.T) {
		t.Setenv(skillSpaceScopeEnv, "true")
		for name, token := range map[string]func(mcpTokenSigner) string{"member": writeToken, "admin": adminToken} {
			database := &recordingDatabase{}
			response := serveSkill(t, database, token, http.MethodPost, "/api/v1/skills",
				`{"name":"room skill","content":"body","scope":"space","scope_id":"room-1"}`)
			requireSpaceDecisionRefusal(t, response.Code, response.Body.Bytes())
			if database.databaseCalls() != 0 {
				t.Fatalf("%s: a refused create must never reach the database, got %d calls", name, database.databaseCalls())
			}
		}
	})
	for _, test := range skillEditAndDelete {
		t.Run(test.name+" of a Space skill", func(t *testing.T) {
			for name, token := range map[string]func(mcpTokenSigner) string{"member": writeToken, "admin": adminToken} {
				database := ownedSkillDatabase(spaceSkillScope, "")
				response := serveSkill(t, database, token, test.method, "/api/v1/skills/sk-1", test.body)
				requireSpaceDecisionRefusal(t, response.Code, response.Body.Bytes())
				if len(database.execs) != 0 || database.commits != 0 {
					t.Fatalf("%s: a refused change must write and commit nothing: execs=%#v commits=%d", name, database.execs, database.commits)
				}
			}
		})
	}
}
