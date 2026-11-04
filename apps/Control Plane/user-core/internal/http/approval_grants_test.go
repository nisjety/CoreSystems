package http

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S2: the gateway-delegated approval routes. The pure tier covers the
// typed envelope, the pre-database refusals and the guard; the legs that need a
// database are in approval_grants_live_test.go.

type approvalErrorBody struct {
	Error struct {
		Code    string         `json:"code"`
		Message string         `json:"message"`
		Details map[string]any `json:"details"`
	} `json:"error"`
}

func decodeApprovalError(t *testing.T, rec *httptest.ResponseRecorder) approvalErrorBody {
	t.Helper()
	var body approvalErrorBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("body is not the typed envelope: %v (%s)", err, rec.Body.String())
	}
	if body.Error.Code == "" || body.Error.Details == nil {
		t.Fatalf("typed envelope needs a code and an object details: %s", rec.Body.String())
	}
	return body
}

func wantApprovalError(t *testing.T, rec *httptest.ResponseRecorder, status int, code string) approvalErrorBody {
	t.Helper()
	if rec.Code != status {
		t.Fatalf("status = %d, want %d (%s)", rec.Code, status, rec.Body.String())
	}
	body := decodeApprovalError(t, rec)
	if body.Error.Code != code {
		t.Fatalf("code = %q, want %q (%s)", body.Error.Code, code, rec.Body.String())
	}
	return body
}

// approvalHarness serves the registered Space routes with a fixed auth context.
func approvalHarness(t *testing.T, repo *spaces.Repository, resolver RunActionAuthorityResolver, ctx map[string]any) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetSpaceRepository(repo)
	if resolver != nil {
		server.SetRunActionAuthorityResolver(resolver)
	}
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	group.Use(func(c *gin.Context) {
		for key, value := range ctx {
			c.Set(key, value)
		}
		c.Next()
	})
	server.registerSpaceRoutes(group)
	return engine
}

func doJSON(engine *gin.Engine, method, path string, body any) *httptest.ResponseRecorder {
	var raw []byte
	switch typed := body.(type) {
	case nil:
	case string:
		raw = []byte(typed)
	default:
		raw, _ = json.Marshal(typed)
	}
	req := httptest.NewRequest(method, path, strings.NewReader(string(raw)))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	engine.ServeHTTP(rec, req)
	return rec
}

func TestWriteApprovalErrorMapsEveryRefusalToItsTypedCode(t *testing.T) {
	gin.SetMode(gin.TestMode)
	for _, tc := range []struct {
		name   string
		err    error
		status int
		code   string
	}{
		{"no membership", spaces.ErrNoCurrentMembership, http.StatusNotFound, "space_not_found"},
		{"wrapped no membership", fmt.Errorf("resolve: %w", spaces.ErrNoCurrentMembership), http.StatusNotFound, "space_not_found"},
		{"scope", spaces.ErrApprovalScopeNotPermitted, http.StatusForbidden, "approval_scope_not_permitted"},
		{"rule not found", spaces.ErrApprovalRuleNotFound, http.StatusNotFound, "approval_grant_not_found"},
		{"agent not in space", spaces.ErrApprovalAgentNotInSpace, http.StatusConflict, "approval_agent_not_in_space"},
		{"policy conflict", spaces.ErrApprovalPolicyConflict, http.StatusConflict, "approval_policy_revision_conflict"},
		{"idempotency", spaces.ErrApprovalIdempotencyReused, http.StatusConflict, "idempotency_key_reused"},
		{"entitlement off", fmt.Errorf("x: %w", approvalDenial(t, "entitlement")), http.StatusForbidden, "approval_grants_not_entitled"},
		{"role below floor", approvalDenial(t, "role"), http.StatusForbidden, "approval_scope_not_permitted"},
		{"zdr", approvalDenial(t, "zdr"), http.StatusPreconditionFailed, "zdr_durable_mutation_forbidden"},
		{"other denial keeps its own code", approvalDenial(t, "kind"), http.StatusForbidden, string(spaces.DenialKindUnsupported)},
		{"unknown error", errors.New("boom"), http.StatusServiceUnavailable, "approval_authority_unavailable"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := httptest.NewRecorder()
			c, _ := gin.CreateTestContext(rec)
			c.Request = httptest.NewRequest(http.MethodGet, "/", nil)
			writeApprovalError(c, tc.err)
			wantApprovalError(t, rec, tc.status, tc.code)
			if strings.Contains(rec.Body.String(), "boom") {
				t.Fatalf("an internal error message leaked: %s", rec.Body.String())
			}
		})
	}
}

// approvalDenial builds a typed denial through the public validators, so the
// test does not depend on unexported constructors.
func approvalDenial(t *testing.T, kind string) error {
	t.Helper()
	evidence := spaces.PersonalThreadDecisionEvidence{}
	evidence.Membership = spaces.CurrentMembership{
		SpaceRef: "s", OrgID: "o", SubjectID: "u", Kind: spaces.KindRoom, Role: "manager",
		Revisions: spaces.AuthorityRevision{Authority: 1, Membership: 1, Privacy: 1, RecipientAudience: 1, Entitlement: 1},
	}
	evidence.RecipientAudienceRef, evidence.RecipientAudienceHash = "a", "sha256:a"
	evidence.ResourceAuthorizationRef = "r"
	evidence.ApprovalGrantEntitled = true
	evidence.Privacy = spaces.PrivacyPolicySnapshot{
		PolicyRef: "p", Purpose: "assistant_collaboration", LawfulBasis: "contract", PrivacyClass: "internal",
		RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
	}
	switch kind {
	case "entitlement":
		evidence.ApprovalGrantEntitled = false
	case "role":
		evidence.Membership.Role = "viewer"
	case "zdr":
		evidence.Privacy.ZeroDataRetention = true
	case "kind":
		evidence.Membership.Kind = spaces.KindPersonal
	}
	err := evidence.ValidateForApprovalRuleManage()
	if _, ok := spaces.AsDenial(err); !ok {
		t.Fatalf("%s: expected a typed denial, got %v", kind, err)
	}
	return err
}

func validCreateApprovalGrantBody() map[string]any {
	return map[string]any{
		"space_ref": "room-1", "scope": "always", "agent_subject_id": "agent-1",
		"tool_key": "mcp__github__list_issues", "action_key": "mcp__github__list_issues",
		"action_schema_hash": "sha256:" + strings.Repeat("a", 64), "idempotency_key": "grant-create-001",
	}
}

func TestApprovalManagerRoutesRequireAVerifiedGatewayDelegation(t *testing.T) {
	routes := []struct{ method, path string }{
		{http.MethodPost, "/api/v1/internal/spaces/approval-grants"},
		{http.MethodGet, "/api/v1/internal/spaces/room-1/approval-grants"},
		{http.MethodDelete, "/api/v1/internal/spaces/room-1/approval-grants/apr_1"},
		{http.MethodGet, "/api/v1/internal/spaces/room-1/approval-policy"},
		{http.MethodPut, "/api/v1/internal/spaces/room-1/approval-policy"},
	}
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"execution core":      {"auth_method": "service_principal", "service_id": executionCorePrincipal, "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		engine := approvalHarness(t, &spaces.Repository{}, nil, ctx)
		for _, route := range routes {
			t.Run(name+" "+route.method+" "+route.path, func(t *testing.T) {
				rec := doJSON(engine, route.method, route.path, validCreateApprovalGrantBody())
				if rec.Code != http.StatusForbidden {
					t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
				}
			})
		}
	}
}

func TestCreateApprovalGrantRefusesBeforeTheDatabase(t *testing.T) {
	engine := approvalHarness(t, &spaces.Repository{}, nil, gatewayContext())
	post := func(mutate func(map[string]any)) *httptest.ResponseRecorder {
		body := validCreateApprovalGrantBody()
		if mutate != nil {
			mutate(body)
		}
		return doJSON(engine, http.MethodPost, "/api/v1/internal/spaces/approval-grants", body)
	}

	// Control stores standing rules only: once and thread are Session Core's.
	for _, scope := range []string{"once", "thread", "forever"} {
		wantApprovalError(t, post(func(b map[string]any) { b["scope"] = scope }), http.StatusForbidden, "approval_scope_not_permitted")
	}
	// A tool a standing rule may never cover is the same typed refusal.
	for _, tool := range []string{"execute_provider_action:gmail.send", "tickets.create", "shell", "browser.click"} {
		wantApprovalError(t, post(func(b map[string]any) { b["tool_key"], b["action_key"] = tool, tool }), http.StatusForbidden, "approval_scope_not_permitted")
	}
	// Shape errors are a 400, with no authority lookup.
	for name, mutate := range map[string]func(map[string]any){
		"missing agent":     func(b map[string]any) { delete(b, "agent_subject_id") },
		"short key":         func(b map[string]any) { b["idempotency_key"] = "x" },
		"bad schema hash":   func(b map[string]any) { b["action_schema_hash"] = "nope" },
		"unknown field":     func(b map[string]any) { b["role"] = "owner" },
		"actor in the body": func(b map[string]any) { b["user_id"] = "someone-else" },
		"org in the body":   func(b map[string]any) { b["org_id"] = "org-2" },
		"revision in body":  func(b map[string]any) { b["authority_revision"] = 99 },
	} {
		t.Run(name, func(t *testing.T) {
			wantApprovalError(t, post(mutate), http.StatusBadRequest, "invalid_request")
		})
	}
	wantApprovalError(t, doJSON(engine, http.MethodPost, "/api/v1/internal/spaces/approval-grants", "{not json"), http.StatusBadRequest, "invalid_request")
	wantApprovalError(t,
		doJSON(engine, http.MethodPost, "/api/v1/internal/spaces/approval-grants", `{"space_ref":"`+strings.Repeat("a", maxApprovalBodyBytes)+`"}`),
		http.StatusBadRequest, "invalid_request")
}

func TestApprovalRoutesFailClosedWithoutARepository(t *testing.T) {
	engine := approvalHarness(t, nil, nil, gatewayContext())
	for _, route := range []struct {
		method, path string
		body         any
	}{
		{http.MethodPost, "/api/v1/internal/spaces/approval-grants", validCreateApprovalGrantBody()},
		{http.MethodGet, "/api/v1/internal/spaces/room-1/approval-grants", nil},
		{http.MethodDelete, "/api/v1/internal/spaces/room-1/approval-grants/apr_1", nil},
		{http.MethodGet, "/api/v1/internal/spaces/room-1/approval-policy", nil},
		{http.MethodPut, "/api/v1/internal/spaces/room-1/approval-policy", map[string]any{}},
	} {
		wantApprovalError(t, doJSON(engine, route.method, route.path, route.body), http.StatusServiceUnavailable, "approval_authority_unavailable")
	}
}

func TestPutApprovalPolicyRejectsAnInvalidPolicyBeforeAuthority(t *testing.T) {
	engine := approvalHarness(t, &spaces.Repository{}, nil, gatewayContext())
	for name, body := range map[string]any{
		"unknown scope":      map[string]any{"max_grant_scope": "forever", "default_addressee_kind": "requester", "quarantine_mode": "off"},
		"unknown addressee":  map[string]any{"max_grant_scope": "once", "default_addressee_kind": "all", "quarantine_mode": "off"},
		"unknown quarantine": map[string]any{"max_grant_scope": "once", "default_addressee_kind": "requester", "quarantine_mode": "x"},
		"negative revision":  map[string]any{"max_grant_scope": "once", "default_addressee_kind": "requester", "quarantine_mode": "off", "expected_revision": -1},
		"unknown field":      map[string]any{"max_grant_scope": "once", "default_addressee_kind": "requester", "quarantine_mode": "off", "org_id": "o"},
	} {
		t.Run(name, func(t *testing.T) {
			wantApprovalError(t, doJSON(engine, http.MethodPut, "/api/v1/internal/spaces/room-1/approval-policy", body), http.StatusBadRequest, "invalid_request")
		})
	}
}
