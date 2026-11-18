package http

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

// WS17-S4: the personal agent registry (/api/v1/internal/agents/subjects) and
// the room agent routes (/api/v1/internal/spaces/{ref}/agent-*). A fake
// delegation stands in for the gateway; the real routes (registered through
// the P-UC1 seam) and the real requireGatewayDelegation guard run. Success
// paths need *spaces.Repository's own Postgres pool, so they are covered by
// internal/spaces's live tests, not here.

const (
	agentSubjectsPath   = "/api/v1/internal/agents/subjects"
	agentSubjectPath    = "/api/v1/internal/agents/subjects/agent-sample1"
	agentAccessPath     = "/api/v1/internal/agents/subjects/agent-sample1/access"
	agentCandidatesPath = "/api/v1/internal/spaces/sample-1/agent-candidates"
	agentAdmissionsPath = "/api/v1/internal/spaces/sample-1/agent-admissions"
	agentWithdrawPath   = "/api/v1/internal/spaces/sample-1/agent-admissions/agent-sample1"
)

// agentsEngine registers both agent route families (the registry, through
// featureRouteRegistrars, and the room routes, through
// spaceFeatureRouteRegistrars) on one engine with no auth middleware -- used
// only to count routes.
func agentsEngine(s *Server) *gin.Engine {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	v1 := engine.Group("/api/v1")
	internal := v1.Group("/internal")
	for _, register := range featureRouteRegistrars {
		register(s, v1, internal)
	}
	spacesGroup := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(s, spacesGroup)
	}
	return engine
}

// agentsCall builds a fresh engine with the caller's auth context set before
// the routes are registered (so the real requireGatewayDelegation guard sees
// it), then dispatches one request. Mirrors orgPolicyRig.do and directoryCall.
func agentsCall(s *Server, caller orgPolicyCaller, method, target, body string) *httptest.ResponseRecorder {
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
		register(s, v1, internal)
	}
	spacesGroup := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(s, spacesGroup)
	}
	request := httptest.NewRequest(method, target, bytes.NewBufferString(body))
	request.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	engine.ServeHTTP(w, request)
	return w
}

func TestAgentRoutesRegisterThroughTheSeam(t *testing.T) {
	engine := agentsEngine(&Server{})
	counts := routeCounts(engine.Routes())
	// gin.Routes() reports the route template, with its own param names, not a
	// concrete path -- unlike agentSubjectPath etc, which are concrete request
	// targets for the other tests below.
	for _, route := range []string{
		"POST /api/v1/internal/agents/subjects",
		"GET /api/v1/internal/agents/subjects",
		"GET /api/v1/internal/agents/subjects/:agent_subject_id",
		"PUT /api/v1/internal/agents/subjects/:agent_subject_id/access",
		"POST /api/v1/internal/spaces/:space_ref/agent-candidates",
		"POST /api/v1/internal/spaces/:space_ref/agent-admissions",
		"DELETE /api/v1/internal/spaces/:space_ref/agent-admissions/:agent_subject_id",
	} {
		if counts[route] != 1 {
			t.Errorf("route %q registered %d times", route, counts[route])
		}
	}
}

// TestAgentServiceScopeTableCoversNewPaths: every /api/v1/internal/agents/**
// path resolves to agents:manage, every room-level agent path resolves to
// spaces:agents:manage, and neither ever falls back to a users:* default
// (named distinctly from service_auth_test.go's own
// TestServiceScopeTableCoversNewPaths, which only walks the spaces:* prefix).
func TestAgentServiceScopeTableCoversNewPaths(t *testing.T) {
	cases := []struct {
		method, path, want string
	}{
		{http.MethodPost, agentSubjectsPath, agentsManageScope},
		{http.MethodGet, agentSubjectsPath, agentsManageScope},
		{http.MethodGet, agentSubjectPath, agentsManageScope},
		{http.MethodPut, agentAccessPath, agentsManageScope},
		{http.MethodPost, agentCandidatesPath, spaceAgentsManageScope},
		{http.MethodPost, agentAdmissionsPath, spaceAgentsManageScope},
		{http.MethodDelete, agentWithdrawPath, spaceAgentsManageScope},
	}
	for _, tc := range cases {
		request := httptest.NewRequest(tc.method, tc.path, nil)
		got := serviceScopeForRequest(request)
		if got != tc.want {
			t.Errorf("%s %s: scope = %q, want %q", tc.method, tc.path, got, tc.want)
		}
		if strings.HasPrefix(got, "users:") {
			t.Errorf("%s %s: resolved to a users:* scope %q", tc.method, tc.path, got)
		}
		// A caller header must not change an agent route's scope either.
		request.Header.Set("X-User-Id", "subject")
		if again := serviceScopeForRequest(request); again != got {
			t.Errorf("%s %s: scope %q changes to %q with X-User-Id", tc.method, tc.path, got, again)
		}
	}
}

func TestAgentRoutesRequireGatewayDelegation(t *testing.T) {
	registry := gatewayAdmin(agentsManageScope)
	room := gatewayAdmin(spaceAgentsManageScope)
	cases := []struct {
		name   string
		caller orgPolicyCaller
		method string
		target string
	}{
		{"no delegation (registry)", func() orgPolicyCaller { c := registry; c.delegated = false; return c }(), http.MethodGet, agentSubjectsPath},
		{"another service principal (registry)", func() orgPolicyCaller { c := registry; c.serviceID = "integration-core"; return c }(), http.MethodGet, agentSubjectsPath},
		{"not a service principal (registry)", func() orgPolicyCaller { c := registry; c.authMethod = "user"; return c }(), http.MethodGet, agentSubjectsPath},
		{"missing scope (registry)", gatewayAdmin("agents:read"), http.MethodGet, agentSubjectsPath},
		{"no user (registry)", func() orgPolicyCaller { c := registry; c.userID = ""; return c }(), http.MethodGet, agentSubjectsPath},
		{"no org (registry)", func() orgPolicyCaller { c := registry; c.orgID = " "; return c }(), http.MethodGet, agentSubjectsPath},
		{"no auth context at all (registry)", orgPolicyCaller{omitContext: true}, http.MethodGet, agentSubjectsPath},
		{"room scope does not open the registry route", room, http.MethodGet, agentSubjectsPath},

		{"no delegation (room)", func() orgPolicyCaller { c := room; c.delegated = false; return c }(), http.MethodPost, agentCandidatesPath},
		{"another service principal (room)", func() orgPolicyCaller { c := room; c.serviceID = "integration-core"; return c }(), http.MethodPost, agentCandidatesPath},
		{"not a service principal (room)", func() orgPolicyCaller { c := room; c.authMethod = "user"; return c }(), http.MethodPost, agentCandidatesPath},
		{"missing scope (room)", gatewayAdmin("spaces:read"), http.MethodPost, agentCandidatesPath},
		{"no user (room)", func() orgPolicyCaller { c := room; c.userID = ""; return c }(), http.MethodPost, agentCandidatesPath},
		{"no org (room)", func() orgPolicyCaller { c := room; c.orgID = " "; return c }(), http.MethodPost, agentCandidatesPath},
		{"no auth context at all (room)", orgPolicyCaller{omitContext: true}, http.MethodPost, agentCandidatesPath},
		{"registry scope does not open the room route", registry, http.MethodPost, agentCandidatesPath},
	}
	for _, tc := range cases {
		s := &Server{}
		w := agentsCall(s, tc.caller, tc.method, tc.target, "{}")
		if w.Code != http.StatusForbidden {
			t.Errorf("%s: want 403, got %d (%s)", tc.name, w.Code, w.Body.String())
			continue
		}
		var body struct {
			Error struct {
				Code    string         `json:"code"`
				Details map[string]any `json:"details"`
			} `json:"error"`
		}
		if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
			t.Fatalf("%s: not a typed error envelope: %v (%s)", tc.name, err, w.Body.String())
		}
		if body.Error.Code != "agent_delegation_required" {
			t.Errorf("%s: code = %q, want agent_delegation_required", tc.name, body.Error.Code)
		}
		if body.Error.Details == nil {
			t.Errorf("%s: details must be an object, never null", tc.name)
		}
	}
}

// TestUnadmittedListIsAlwaysAJSONArray: the membership-replacement response's
// skipped_unadmitted field (spaces.go) is always an array, never null, even
// when nothing was skipped.
func TestUnadmittedListIsAlwaysAJSONArray(t *testing.T) {
	for _, tc := range []struct {
		name string
		in   []string
		want string
	}{
		{"nil (nothing skipped)", nil, "[]"},
		{"empty slice", []string{}, "[]"},
		{"one id", []string{"agent-x"}, `["agent-x"]`},
	} {
		got := unadmittedList(tc.in)
		if got == nil {
			t.Errorf("%s: unadmittedList returned nil, must never marshal to null", tc.name)
		}
		encoded, err := json.Marshal(got)
		if err != nil {
			t.Fatalf("%s: marshal: %v", tc.name, err)
		}
		if string(encoded) != tc.want {
			t.Errorf("%s: json = %s, want %s", tc.name, encoded, tc.want)
		}
	}
}
