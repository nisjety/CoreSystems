package http

import (
	"bytes"
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// W3-DEC-ORGAGENT (D19): the org-admin routes of the org-visible agent
// definitions (space_agents_org.go). A fake role resolver stands in for
// auth-core, through the same spaceOrgPolicy.roleResolver seam
// space_org_policy_test.go uses (orgAdminRole is shared). Success paths need
// *spaces.Repository's own Postgres pool, so they are covered by
// internal/spaces's live tests, not here.

const (
	orgAgentsListPath      = "/api/v1/internal/agents/org-subjects"
	orgAgentSuspendPath    = "/api/v1/internal/agents/org-subjects/agent-sample1/suspend"
	orgAgentReactivatePath = "/api/v1/internal/agents/org-subjects/agent-sample1/reactivate"
	orgAgentRetirePath     = "/api/v1/internal/agents/org-subjects/agent-sample1"
)

type orgAgentRig struct {
	server    *Server
	role      string
	roleErr   error
	roleCalls int
}

// newOrgAgentRig wires a non-nil *spaces.Repository (with no database pool)
// so requireOrgAgentAdmin's "s.spaceRepo == nil" early-out never fires in
// these guard tests; every case here is refused before the repository would
// ever be touched.
func newOrgAgentRig() *orgAgentRig {
	rig := &orgAgentRig{role: "admin"}
	rig.server = &Server{spaceRepo: spaces.NewRepository(nil)}
	rig.server.spaceOrgPolicy = spaceOrgPolicyConfig{
		roleResolver: func(context.Context, string, string) (string, error) {
			rig.roleCalls++
			return rig.role, rig.roleErr
		},
	}
	return rig
}

func (r *orgAgentRig) do(caller orgPolicyCaller, method, target, body string) *httptest.ResponseRecorder {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	engine.Use(func(c *gin.Context) {
		if caller.omitContext {
			return
		}
		c.Set("auth_method", caller.authMethod)
		c.Set("service_id", caller.serviceID)
		c.Set("delegation_verified", caller.delegated)
		c.Set("service_scopes", caller.scopes)
		c.Set("user_id", caller.userID)
		c.Set("org_id", caller.orgID)
	})
	v1 := engine.Group("/api/v1")
	internal := v1.Group("/internal")
	for _, register := range featureRouteRegistrars {
		register(r.server, v1, internal)
	}
	request := httptest.NewRequest(method, target, bytes.NewBufferString(body))
	request.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	engine.ServeHTTP(w, request)
	return w
}

func TestOrgAgentRoutesRegisterThroughTheSeam(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	v1 := engine.Group("/api/v1")
	internal := v1.Group("/internal")
	for _, register := range featureRouteRegistrars {
		register(&Server{}, v1, internal)
	}
	counts := routeCounts(engine.Routes())
	for _, route := range []string{
		"GET " + orgAgentsListPath,
		"PUT /api/v1/internal/agents/org-subjects/:agent_subject_id/suspend",
		"PUT /api/v1/internal/agents/org-subjects/:agent_subject_id/reactivate",
		"DELETE /api/v1/internal/agents/org-subjects/:agent_subject_id",
	} {
		if counts[route] != 1 {
			t.Errorf("route %q registered %d times", route, counts[route])
		}
	}
}

// TestOrgAgentAdminRefusesNonAdminRoles: a member (or any non-owner/admin
// canonical role) is refused on all four routes, and the repository is never
// reached.
func TestOrgAgentAdminRefusesNonAdminRoles(t *testing.T) {
	calls := []struct{ method, target, body string }{
		{http.MethodGet, orgAgentsListPath, ""},
		{http.MethodPut, orgAgentSuspendPath, `{"expected_revision":1}`},
		{http.MethodPut, orgAgentReactivatePath, `{"expected_revision":1}`},
		{http.MethodDelete, orgAgentRetirePath, `{"expected_revision":1}`},
	}
	for _, role := range []string{"member", "viewer", "", "guest", "Admin", "owner "} {
		for _, call := range calls {
			rig := newOrgAgentRig()
			rig.role = role
			w := rig.do(gatewayAdmin(agentsManageScope), call.method, call.target, call.body)
			if w.Code != http.StatusForbidden {
				t.Fatalf("role %q %s %s: want 403, got %d (%s)", role, call.method, call.target, w.Code, w.Body.String())
			}
			if code, _, _ := orgPolicyError(t, w); code != "space_org_admin_required" {
				t.Fatalf("role %q %s %s: code %q", role, call.method, call.target, code)
			}
		}
	}
	// owner and admin pass the role gate (list reaches the repository, which
	// has no database and so answers 503 from inside ListOrgAgents itself --
	// still proof the guard let them through).
	for _, role := range []string{"owner", "admin"} {
		rig := newOrgAgentRig()
		rig.role = role
		w := rig.do(gatewayAdmin(agentsManageScope), http.MethodGet, orgAgentsListPath, "")
		if w.Code == http.StatusForbidden {
			t.Fatalf("role %q: a canonical admin was refused at the role gate (%d %s)", role, w.Code, w.Body.String())
		}
	}
}

// TestOrgAgentAdminRequiresGatewayDelegationScopeAndIdentity: a missing
// agents:manage scope or a non-gateway principal is refused the same way as a
// non-admin role, and the role resolver is never called.
func TestOrgAgentAdminRequiresGatewayDelegationScopeAndIdentity(t *testing.T) {
	base := gatewayAdmin(agentsManageScope)
	cases := []struct {
		name   string
		caller orgPolicyCaller
	}{
		{"no delegation", func() orgPolicyCaller { c := base; c.delegated = false; return c }()},
		{"another service principal", func() orgPolicyCaller { c := base; c.serviceID = "integration-core"; return c }()},
		{"not a service principal", func() orgPolicyCaller { c := base; c.authMethod = "user"; return c }()},
		{"missing agents:manage scope", gatewayAdmin("agents:read")},
		{"a spaces:* scope is not agents:manage", gatewayAdmin(spaceAgentsManageScope)},
		{"no user", func() orgPolicyCaller { c := base; c.userID = ""; return c }()},
		{"no org", func() orgPolicyCaller { c := base; c.orgID = " "; return c }()},
		{"no auth context at all", orgPolicyCaller{omitContext: true}},
	}
	for _, tc := range cases {
		rig := newOrgAgentRig()
		w := rig.do(tc.caller, http.MethodGet, orgAgentsListPath, "")
		if w.Code != http.StatusForbidden {
			t.Errorf("%s: want 403, got %d (%s)", tc.name, w.Code, w.Body.String())
			continue
		}
		if code, _, _ := orgPolicyError(t, w); code != "space_org_admin_required" {
			t.Errorf("%s: code %q", tc.name, code)
		}
		if rig.roleCalls != 0 {
			t.Errorf("%s: a refused caller reached the role resolver (%d calls)", tc.name, rig.roleCalls)
		}
	}
}

// TestOrgAgentAdminMembershipAuthorityFailuresFailClosed: a membership
// authority outage answers 503, and "not a member at all" answers a
// different 403 code than a wrong role.
func TestOrgAgentAdminMembershipAuthorityFailuresFailClosed(t *testing.T) {
	rig := newOrgAgentRig()
	rig.roleErr = errors.New("auth-core unreachable")
	w := rig.do(gatewayAdmin(agentsManageScope), http.MethodGet, orgAgentsListPath, "")
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("auth-core down: want 503, got %d (%s)", w.Code, w.Body.String())
	}
	if code, _, _ := orgPolicyError(t, w); code != "space_policy_membership_authority_unavailable" {
		t.Fatalf("code = %q", code)
	}

	rig = newOrgAgentRig()
	rig.roleErr = ErrMembershipNotFound
	w = rig.do(gatewayAdmin(agentsManageScope), http.MethodGet, orgAgentsListPath, "")
	if w.Code != http.StatusForbidden {
		t.Fatalf("not a member: want 403, got %d", w.Code)
	}
	if code, _, _ := orgPolicyError(t, w); code != "space_org_membership_required" {
		t.Fatalf("code = %q", code)
	}
}

// TestOrgAgentRoutesRejectMissingExpectedRevision: each write route requires
// expected_revision in the body; a canonical admin without it gets 400
// invalid_request before the repository is ever touched.
func TestOrgAgentRoutesRejectMissingExpectedRevision(t *testing.T) {
	for _, call := range []struct{ method, target string }{
		{http.MethodPut, orgAgentSuspendPath},
		{http.MethodPut, orgAgentReactivatePath},
		{http.MethodDelete, orgAgentRetirePath},
	} {
		rig := newOrgAgentRig()
		w := rig.do(gatewayAdmin(agentsManageScope), call.method, call.target, `{}`)
		if w.Code != http.StatusBadRequest {
			t.Errorf("%s %s: want 400, got %d (%s)", call.method, call.target, w.Code, w.Body.String())
			continue
		}
		if code, _, _ := orgPolicyError(t, w); code != spaces.CodeInvalidRequest {
			t.Errorf("%s %s: code %q", call.method, call.target, code)
		}
	}
}
