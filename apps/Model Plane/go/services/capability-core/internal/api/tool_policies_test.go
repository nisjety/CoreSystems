package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// untouchedDB fails the test on any use: every request below must be refused
// before the registry is read or written.
type untouchedDB struct{ t *testing.T }

func (d untouchedDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Helper()
	d.t.Fatal("the registry was written for a request that had to be refused")
	return pgconn.CommandTag{}, nil
}

func (d untouchedDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.t.Helper()
	d.t.Fatal("the registry was read for a request that had to be refused")
	return nil, nil
}

func (d untouchedDB) QueryRow(context.Context, string, ...any) pgx.Row {
	d.t.Helper()
	d.t.Fatal("the registry was read for a request that had to be refused")
	return nil
}

func toolPolicyRequest(method, target, body string, principal *authctx.Principal) *http.Request {
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	if principal != nil {
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), *principal))
	}
	return request
}

func serveToolPolicies(t *testing.T, request *http.Request) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewToolPoliciesHandler(untouchedDB{t}).Register(mux)
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

func person(scopes ...string) authctx.Principal {
	return authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", Scopes: scopes, RetentionPolicyPresent: true}
}

const allowBody = `{"decision":"allow","pinned_digest":"` + knownAnswerToolDigest + `"}`

// Every refusal that does not need the registry is answered before it is read.
func TestToolPolicyWritesAreRefusedBeforeTheRegistryIsTouched(t *testing.T) {
	zdr := person(authz.WriteScope)
	zdr.ZeroDataRetention = true
	unstated := person(authz.WriteScope)
	unstated.RetentionPolicyPresent = false
	workload := person(authz.WriteScope)
	workload.PrincipalType = "service"
	admin := person(authz.WriteScope, "org:admin")
	noWrite := person()
	adminWithoutWrite := person("org:admin")

	tests := []struct {
		name      string
		method    string
		target    string
		body      string
		principal *authctx.Principal
		want      int
		wantBody  string
	}{
		{name: "unauthenticated", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask"}`, want: http.StatusUnauthorized},
		{name: "ZDR principal cannot write", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"never"}`, principal: &zdr, want: http.StatusForbidden, wantBody: "tool_policy_write_under_zdr_refused"},
		{name: "unstated retention posture cannot write", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"never"}`, principal: &unstated, want: http.StatusForbidden, wantBody: "tool_policy_write_under_zdr_refused"},
		{name: "ZDR principal cannot revoke", method: http.MethodDelete, target: "/api/v1/tool-policies/cap.x", principal: &zdr, want: http.StatusForbidden, wantBody: "tool_policy_write_under_zdr_refused"},
		{name: "write needs the capability write scope", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask"}`, principal: &noWrite, want: http.StatusForbidden},
		{name: "an admin scope is not a write scope", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask","scope":"org"}`, principal: &adminWithoutWrite, want: http.StatusForbidden},
		{name: "a workload cannot set a policy", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask"}`, principal: &workload, want: http.StatusForbidden},
		{name: "a non-admin cannot set an org policy", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"never","scope":"org"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusForbidden},
		{name: "a non-admin cannot revoke an org policy", method: http.MethodDelete, target: "/api/v1/tool-policies/cap.x?scope=org", principal: ptr(person(authz.WriteScope)), want: http.StatusForbidden},
		{name: "unknown field", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask","org_id":"org-b"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "trailing JSON", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask"}{}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "bad scope", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask","scope":"thread"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "bad scope on revoke", method: http.MethodDelete, target: "/api/v1/tool-policies/cap.x?scope=thread", principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "bad decision", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"deny"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "an allow needs the reviewed digest", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"allow"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "an allow digest must be sha256 hex", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"allow","pinned_digest":"abc"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "a tightening policy takes no digest", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"never","pinned_digest":"` + knownAnswerToolDigest + `"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "reason is bounded", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"ask","reason":"` + strings.Repeat("x", 501) + `"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusBadRequest},
		{name: "an admin org write still validates the body", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x", body: `{"decision":"allow","scope":"org"}`, principal: &admin, want: http.StatusBadRequest},
		{name: "list rejects a bad capability id", method: http.MethodGet, target: "/api/v1/tool-policies?capability_id=a/b", principal: ptr(person()), want: http.StatusBadRequest},
		{name: "list needs authentication", method: http.MethodGet, target: "/api/v1/tool-policies", want: http.StatusUnauthorized},
		{name: "the collection is read only", method: http.MethodPut, target: "/api/v1/tool-policies", principal: ptr(person(authz.WriteScope)), want: http.StatusMethodNotAllowed},
		{name: "an item cannot be POSTed", method: http.MethodPost, target: "/api/v1/tool-policies/cap.x", principal: ptr(person(authz.WriteScope)), want: http.StatusMethodNotAllowed},
		{name: "a nested path is not a capability id", method: http.MethodPut, target: "/api/v1/tool-policies/cap.x/y", principal: ptr(person(authz.WriteScope)), want: http.StatusNotFound},
		{name: "an ask on the provider write is already always ask", method: http.MethodPut, target: "/api/v1/tool-policies/cap.tool.provider.execute", body: `{"decision":"ask"}`, principal: ptr(person(authz.WriteScope)), want: http.StatusUnprocessableEntity, wantBody: "already_always_ask"},
		{name: "an org ask on the provider write is already always ask", method: http.MethodPut, target: "/api/v1/tool-policies/cap.tool.provider.execute", body: `{"decision":"ask","scope":"org"}`, principal: &admin, want: http.StatusUnprocessableEntity, wantBody: "already_always_ask"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			response := serveToolPolicies(t, toolPolicyRequest(tc.method, tc.target, tc.body, tc.principal))
			if response.Code != tc.want {
				t.Fatalf("status = %d, want %d, body=%s", response.Code, tc.want, response.Body.String())
			}
			if tc.wantBody != "" && !strings.Contains(response.Body.String(), tc.wantBody) {
				t.Fatalf("body = %s, want it to contain %q", response.Body.String(), tc.wantBody)
			}
		})
	}
}

func ptr[T any](v T) *T { return &v }

// builtinToolDB answers the capability read as a built-in tool, so the connected-app
// capabilities are written against the kind they have in the registry.
type builtinToolDB struct{ *toolPolicyRecordingDB }

func (d builtinToolDB) QueryRow(ctx context.Context, sql string, args ...any) pgx.Row {
	if strings.Contains(sql, "FROM capabilities") {
		d.statements = append(d.statements, recordedStatement{sql: sql, args: args})
		return toolPolicyScanRow{values: []string{models.KindTool}}
	}
	return d.toolPolicyRecordingDB.QueryRow(ctx, sql, args...)
}

// putConnectedAppLevel sends one person's PUT for a connected-app capability and
// returns the response and what the database was asked to do.
func putConnectedAppLevel(t *testing.T, capabilityID, body string) (*httptest.ResponseRecorder, *toolPolicyRecordingDB) {
	t.Helper()
	db := &toolPolicyRecordingDB{t: t}
	mux := http.NewServeMux()
	NewToolPoliciesHandler(builtinToolDB{db}).Register(mux)
	principal := person(authz.WriteScope)
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, toolPolicyRequest(http.MethodPut, "/api/v1/tool-policies/"+capabilityID, body, &principal))
	return response, db
}

// The connected-app levels a person may pick are tightenings (EP-10 S7): ask first
// or off for the read, off for the write. Each is stored as the person's own row.
func TestConnectedAppLevelsAreTighteningsAPersonMaySet(t *testing.T) {
	levels := []struct{ capabilityID, decision string }{
		{"cap.tool.provider.read", "ask"},
		{"cap.tool.provider.read", "never"},
		{"cap.tool.provider.execute", "never"},
	}
	for _, level := range levels {
		t.Run(level.capabilityID+" "+level.decision, func(t *testing.T) {
			response, db := putConnectedAppLevel(t, level.capabilityID, `{"decision":"`+level.decision+`"}`)
			if response.Code != http.StatusOK {
				t.Fatalf("status = %d, want %d, body=%s", response.Code, http.StatusOK, response.Body.String())
			}
			args := policyWriteArgs(t, db)
			if args[1] != "user" || args[2] != "user-a" || args[3] != level.capabilityID || args[4] != level.decision {
				t.Fatalf("stored scope=%v value=%v capability=%v decision=%v", args[1], args[2], args[3], args[4])
			}
		})
	}
}

// A connected-app capability is built in, so it is never relaxed to allow.
func TestAConnectedAppCapabilityIsNeverAllowed(t *testing.T) {
	for _, capabilityID := range []string{"cap.tool.provider.read", "cap.tool.provider.execute"} {
		response, db := putConnectedAppLevel(t, capabilityID, allowBody)
		if response.Code != http.StatusUnprocessableEntity {
			t.Fatalf("%s: status = %d, want %d, body=%s", capabilityID, response.Code, http.StatusUnprocessableEntity, response.Body.String())
		}
		for _, statement := range db.statements {
			if statement.sql == setToolPolicySQL {
				t.Fatalf("%s: an allow on a built-in tool was written", capabilityID)
			}
		}
	}
}

// listRowsDB answers the list read with no rows; nothing else may be asked.
type listRowsDB struct{ untouchedDB }

func (listRowsDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	return emptyRows{}, nil
}

// The list says whether the engine honours the policies, and says false unless
// main recorded otherwise, so a level is never shown as honoured on a guess.
func TestTheListSaysWhetherThePoliciesAreEnforced(t *testing.T) {
	for _, enforced := range []bool{false, true} {
		mux := http.NewServeMux()
		handler := NewToolPoliciesHandler(listRowsDB{untouchedDB{t}})
		if enforced {
			handler = handler.WithEnforced(true)
		}
		handler.Register(mux)
		response := httptest.NewRecorder()
		mux.ServeHTTP(response, toolPolicyRequest(http.MethodGet, "/api/v1/tool-policies?capability_id=cap.tool.provider.read", "", ptr(person())))
		if response.Code != http.StatusOK {
			t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
		}
		var body struct {
			Policies []toolPolicyView `json:"policies"`
			Enforced *bool            `json:"enforced"`
		}
		if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
			t.Fatalf("the list is not JSON: %v", err)
		}
		if body.Enforced == nil || *body.Enforced != enforced {
			t.Fatalf("enforced = %v, want %v: %s", body.Enforced, enforced, response.Body.String())
		}
		if body.Policies == nil || !strings.Contains(response.Body.String(), `"policies":[]`) {
			t.Fatalf("an empty list must marshal as []: %s", response.Body.String())
		}
	}
}

// The admin test used for an org-scope write is the same one the model-gateway
// applies (ownership.rs is_admin_claim): these are the vectors both sides must
// give the same answer for.
func TestAdminScopeVectorsMatchTheGateway(t *testing.T) {
	vectors := []struct {
		scopes []string
		admin  bool
	}{
		{nil, false},
		{[]string{"capability:write"}, false},
		{[]string{"admin"}, true},
		{[]string{"org:admin"}, true},
		{[]string{"*:admin"}, true},
		{[]string{"capability:admin"}, true},
		{[]string{"capability:write", "org:admin"}, true},
		{[]string{"administrator"}, false},
		{[]string{"org:administrator"}, false},
		{[]string{"Admin"}, false},
		{[]string{"ORG:ADMIN"}, false},
		{[]string{" admin"}, false},
		{[]string{"admin "}, false},
		{[]string{"adm:in"}, false},
		{[]string{""}, false},
		{[]string{":admin"}, true},
		{[]string{"admin:"}, false},
	}
	for _, v := range vectors {
		request := toolPolicyRequest(http.MethodGet, "/", "", &authctx.Principal{OrganizationID: "o", ActorID: "a", Scopes: v.scopes})
		if got := verifiedIsAdmin(request); got != v.admin {
			t.Errorf("scopes %q: admin = %v, want %v", v.scopes, got, v.admin)
		}
	}
	if verifiedIsAdmin(httptest.NewRequest(http.MethodGet, "/", nil)) {
		t.Error("a request with no verified principal is an admin")
	}
}
