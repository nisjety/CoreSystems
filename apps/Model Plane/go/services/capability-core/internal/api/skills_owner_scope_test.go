package api

import (
	"net/http"
	"strings"
	"testing"
)

// ownedSkillDatabase answers the ownership read that runs under the skill's row lock with one
// skill of the given scope and owner. Every request in these tests is made as user-a, so an
// owner of user-b makes the skill another member's.
func ownedSkillDatabase(scope, owner string) *recordingDatabase {
	return &recordingDatabase{rows: &skillRows{rows: [][]any{{scope, owner}}}}
}

// skillChangeCase is one change to a skill and the status the caller must get for it.
type skillChangeCase struct {
	name   string
	method string
	body   string
	want   int
}

var skillEditAndDelete = []skillChangeCase{
	{name: "patch", method: http.MethodPatch, body: `{"content":"edited"}`, want: http.StatusOK},
	{name: "delete", method: http.MethodDelete, want: http.StatusNoContent},
}

// A member who holds the id of another member's private skill may not change or delete it. The
// refusal comes before the statement, so nothing is written and nothing is committed.
func TestSkillMemberCannotChangeAnotherUsersSkill(t *testing.T) {
	for _, test := range []skillChangeCase{
		{name: "patch content", method: http.MethodPatch, body: `{"content":"edited"}`, want: http.StatusForbidden},
		{name: "patch description", method: http.MethodPatch, body: `{"description":"edited"}`, want: http.StatusForbidden},
		{name: "delete", method: http.MethodDelete, want: http.StatusForbidden},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := ownedSkillDatabase("user", "user-b")
			response := serveSkill(t, database, writeToken, test.method, "/api/v1/skills/sk-1", test.body)
			if response.Code != test.want {
				t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), test.want)
			}
			if len(database.execs) != 0 || database.commits != 0 {
				t.Fatalf("a refused change must write and commit nothing: execs=%#v commits=%d", database.execs, database.commits)
			}
		})
	}
}

// Enabling another member's undecided proposal answers 403 rather than the proposal's 409, so the
// reply does not reveal that the proposal exists. The proposal is not read at all.
func TestSkillMemberGetsForbiddenNotAProposalAnswer(t *testing.T) {
	database := ownedSkillDatabase("user", "user-b")
	database.execTag = "UPDATE 0"
	database.nextRows = append(database.nextRows, stringRow{value: "proposed"})
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1", `{"enabled":true}`)
	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body=%s, want 403", response.Code, response.Body.String())
	}
	if len(database.queryRows) != 0 || len(database.execs) != 0 {
		t.Fatalf("a refused enable must not read the proposal or run the update: queryRows=%#v execs=%#v", database.queryRows, database.execs)
	}
}

// The owner of a user-scoped skill keeps changing and deleting it. The ownership read takes the
// row lock and is scoped to the skill and to the caller's org.
func TestSkillOwnerCanChangeAndDeleteOwnSkill(t *testing.T) {
	for _, test := range skillEditAndDelete {
		t.Run(test.name, func(t *testing.T) {
			database := ownedSkillDatabase("user", "user-a")
			response := serveSkill(t, database, writeToken, test.method, "/api/v1/skills/sk-1", test.body)
			if response.Code != test.want {
				t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), test.want)
			}
			if len(database.execs) != 1 || database.commits != 1 {
				t.Fatalf("the owner's change must run once and commit: execs=%d commits=%d", len(database.execs), database.commits)
			}
			if len(database.queries) == 0 {
				t.Fatalf("the change must read the skill under its row lock before it writes")
			}
			read := database.queries[0]
			if !strings.Contains(read.query, "FOR UPDATE") || len(read.args) != 2 || read.args[0] != "sk-1" || read.args[1] != "org-a" {
				t.Fatalf("the ownership read must lock the skill and be scoped to its org: query=%q args=%#v", read.query, read.args)
			}
		})
	}
}

// An org admin may change and delete a member's private skill.
func TestSkillAdminCanChangeAnotherUsersSkill(t *testing.T) {
	for _, test := range skillEditAndDelete {
		t.Run(test.name, func(t *testing.T) {
			database := ownedSkillDatabase("user", "user-b")
			response := serveSkill(t, database, adminToken, test.method, "/api/v1/skills/sk-1", test.body)
			if response.Code != test.want {
				t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), test.want)
			}
			if len(database.execs) != 1 || database.commits != 1 {
				t.Fatalf("the admin's change must run once and commit: execs=%d commits=%d", len(database.execs), database.commits)
			}
		})
	}
}

// Org-scoped skills keep today's rule: any member may change or delete them.
func TestSkillOrgScopeStaysOpenToMembers(t *testing.T) {
	for _, test := range skillEditAndDelete {
		t.Run(test.name, func(t *testing.T) {
			database := ownedSkillDatabase("org", "")
			response := serveSkill(t, database, writeToken, test.method, "/api/v1/skills/sk-1", test.body)
			if response.Code != test.want {
				t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), test.want)
			}
			if len(database.execs) != 1 || database.commits != 1 {
				t.Fatalf("a member's change to an org skill must run once and commit: execs=%d commits=%d", len(database.execs), database.commits)
			}
		})
	}
}

// A skill that is not in the caller's org is not a refusal: nothing matches and the answer is 404.
func TestSkillMissingIsNotFoundNotForbidden(t *testing.T) {
	for _, test := range []skillChangeCase{
		{name: "patch", method: http.MethodPatch, body: `{"content":"edited"}`, want: http.StatusNotFound},
		{name: "delete", method: http.MethodDelete, want: http.StatusNotFound},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{execTag: "UPDATE 0"}
			response := serveSkill(t, database, writeToken, test.method, "/api/v1/skills/sk-1", test.body)
			if response.Code != test.want {
				t.Fatalf("status = %d, body=%s, want %d", response.Code, response.Body.String(), test.want)
			}
		})
	}
}
