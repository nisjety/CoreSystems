package http

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

// WS10-S5a: the scope table of the tool policy routes. They are registered
// through the P-UC1 seam, so they only apply to paths the core table did not
// map; without a rule an unmapped PUT would fall through to users:write:self.

func TestToolPolicyRoutePathsMapToTheIssueScope(t *testing.T) {
	const base = "/api/v1/internal/spaces"
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, base + "/space-1/tool-policy"},
		{http.MethodPut, base + "/space-1/tool-policy"},
		{http.MethodPut, base + "/space-1/agents/agent-1/tool-policy"},
	} {
		if got := serviceScopeForRequest(httptest.NewRequest(tc.method, tc.path, nil)); got != "spaces:issue" {
			t.Errorf("%s %s scope = %q, want spaces:issue", tc.method, tc.path, got)
		}
	}
	if !serviceScopeRequiresVerifiedDelegation("spaces:issue") {
		t.Fatal("spaces:issue must stay delegated: a service credential alone never proves the acting user")
	}
}

func TestToolPolicyScopeRulesLeaveOtherMethodsAndShapesUnmapped(t *testing.T) {
	const base = "/api/v1/internal/spaces"
	for _, tc := range []struct{ method, path string }{
		{http.MethodPost, base + "/space-1/tool-policy"},
		{http.MethodPatch, base + "/space-1/tool-policy"},
		{http.MethodDelete, base + "/space-1/tool-policy"},
		{http.MethodGet, base + "/space-1/agents/agent-1/tool-policy"},
		{http.MethodPost, base + "/space-1/agents/agent-1/tool-policy"},
		{http.MethodPut, base + "/space-1/agents/agent-1/tool-policy/extra"},
		{http.MethodPut, base + "/space-1/other/agent-1/tool-policy"},
		{http.MethodPut, base + "/space-1/tool-policies"},
		{http.MethodPut, "/api/v1/internal/other/space-1/tool-policy"},
	} {
		for _, rule := range spaceFeatureScopeRules {
			if scope, ok := rule(tc.method, tc.path); ok && scope == "spaces:issue" && tc.path[len(tc.path)-11:] == "tool-policy" {
				t.Errorf("%s %s must not map to spaces:issue", tc.method, tc.path)
			}
		}
	}
}
