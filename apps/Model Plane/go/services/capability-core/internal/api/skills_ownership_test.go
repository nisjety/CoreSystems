package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

// skillOwnershipRow answers the `SELECT scope, owner_user_id FROM
// agent_skills ...` pre-check the share-patch path runs before mutating
// shared_with, mirroring mcpConfigRow's role for the MCP handler's own
// share-patch test.
type skillOwnershipRow struct {
	scope       string
	ownerUserID string
}

func (row skillOwnershipRow) Scan(dest ...any) error {
	if len(dest) != 2 {
		return errors.New("skillOwnershipRow: want two scan destinations")
	}
	scope, ok := dest[0].(*string)
	if !ok {
		return errors.New("skillOwnershipRow: scope destination is not *string")
	}
	ownerUserID, ok := dest[1].(*string)
	if !ok {
		return errors.New("skillOwnershipRow: owner_user_id destination is not *string")
	}
	*scope, *ownerUserID = row.scope, row.ownerUserID
	return nil
}

func TestSkillCreateOrgScopeRequiresAdmin(t *testing.T) {
	database := &recordingDatabase{}
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPost, "/api/v1/skills",
		strings.NewReader(`{"name":"org skill","content":"body","scope":"org"}`))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body=%s, want 403", response.Code, response.Body.String())
	}
	if database.databaseCalls() != 0 {
		t.Fatalf("rejected create must never reach the database, got %d calls", database.databaseCalls())
	}
}

func TestSkillCreateAdminCanCreateOrgScope(t *testing.T) {
	database := &recordingDatabase{}
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, adminSign := mcpAuthenticatedHandlerWithSigner(t, mux)
	nonZDR := false
	adminToken := adminSign("user", []string{authz.WriteScope, "admin"}, &nonZDR)

	request := httptest.NewRequest(http.MethodPost, "/api/v1/skills",
		strings.NewReader(`{"name":"org skill","content":"body","scope":"org"}`))
	request.Header.Set("Authorization", "Bearer "+adminToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s, want 201", response.Code, response.Body.String())
	}
	if len(database.execs) != 1 {
		t.Fatalf("writes = %d, want one insert", len(database.execs))
	}
	args := database.execs[0].args
	if len(args) != 14 || args[9] != "org" || args[10] != "" {
		t.Fatalf("insert args = %#v, want scope=org and no owner", args)
	}
}

func TestSkillCreateBindsOwnerToCallerNotClientSuppliedValue(t *testing.T) {
	database := &recordingDatabase{}
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPost, "/api/v1/skills",
		strings.NewReader(`{"name":"my skill","content":"body","scope":"user","owner_user_id":"someone-else"}`))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s, want 201", response.Code, response.Body.String())
	}
	args := database.execs[0].args
	if len(args) != 14 || args[9] != "user" || args[10] != "user-a" {
		t.Fatalf("insert args = %#v, want owner bound to the authenticated caller (user-a), never the client-supplied value", args)
	}
}

func TestSkillListPassesCallerAndAdminForVisibilityFiltering(t *testing.T) {
	for _, test := range []struct {
		name      string
		scopes    []string
		wantAdmin bool
	}{
		{name: "member", scopes: nil, wantAdmin: false},
		{name: "admin", scopes: []string{"admin"}, wantAdmin: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			database := &recordingDatabase{}
			skills := NewSkillsHandler(database)
			mux := http.NewServeMux()
			skills.Register(mux)
			handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
			nonZDR := false
			token := sign("user", test.scopes, &nonZDR)

			request := httptest.NewRequest(http.MethodGet, "/api/v1/skills", nil)
			request.Header.Set("Authorization", "Bearer "+token)
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, request)

			if response.Code != http.StatusOK {
				t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
			}
			call := database.queries[len(database.queries)-1]
			if len(call.args) != 3 || call.args[0] != "org-a" || call.args[1] != "user-a" || call.args[2] != test.wantAdmin {
				t.Fatalf("list visibility args = %#v, want [org-a user-a %v]", call.args, test.wantAdmin)
			}
		})
	}
}

func TestSkillShareRequiresOwner(t *testing.T) {
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, skillOwnershipRow{scope: "user", ownerUserID: "someone-else"})
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPatch, "/api/v1/skills/skill-1",
		strings.NewReader(`{"shared_with":["user-b"]}`))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body=%s, want 403", response.Code, response.Body.String())
	}
	if len(database.execs) != 0 {
		t.Fatalf("a non-owner's share attempt must never reach the update, got %d execs", len(database.execs))
	}
}

func TestSkillShareRejectsOrgScope(t *testing.T) {
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, skillOwnershipRow{scope: "org", ownerUserID: ""})
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPatch, "/api/v1/skills/skill-1",
		strings.NewReader(`{"shared_with":["user-b"]}`))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, body=%s, want 422", response.Code, response.Body.String())
	}
}

func TestSkillShareByOwnerCleansGrantees(t *testing.T) {
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, skillOwnershipRow{scope: "user", ownerUserID: "user-a"})
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPatch, "/api/v1/skills/skill-1",
		strings.NewReader(`{"shared_with":["user-a","bob","bob","  ","carol"]}`))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s, want 200", response.Code, response.Body.String())
	}
	call := database.execs[len(database.execs)-1]
	if len(call.args) != 7 || call.args[6] != "org-a" {
		t.Fatalf("share patch was not tenant scoped: args=%v", call.args)
	}
	shared, ok := call.args[3].([]byte)
	var cleaned []string
	if !ok || json.Unmarshal(shared, &cleaned) != nil {
		t.Fatalf("share patch did not persist a decodable shared_with: %#v", call.args[3])
	}
	if len(cleaned) != 2 || cleaned[0] != "bob" || cleaned[1] != "carol" {
		t.Fatalf("shared_with = %v, want [bob carol] (owner-self, blanks, dups dropped)", cleaned)
	}
}

func TestSkillShareUnknownSkillIsNotFound(t *testing.T) {
	database := &recordingDatabase{rowErr: pgx.ErrNoRows}
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPatch, "/api/v1/skills/skill-missing",
		strings.NewReader(`{"shared_with":["user-b"]}`))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
	}
}
