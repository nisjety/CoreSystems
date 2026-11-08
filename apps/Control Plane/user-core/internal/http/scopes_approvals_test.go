package http

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
)

// WS10-S2: the scope table of the approval routes. The rules are registered
// through the P-UC1 seam, so they only apply to paths the core table did not map.

func TestApprovalRoutePathsMapToTheirScopes(t *testing.T) {
	const base = "/api/v1/internal/spaces"
	for _, tc := range []struct {
		method, path, want string
	}{
		{http.MethodPost, base + "/approval-grants", "spaces:issue"},
		{http.MethodGet, base + "/space-1/approval-grants", "spaces:issue"},
		{http.MethodDelete, base + "/space-1/approval-grants/apr_1", "spaces:issue"},
		{http.MethodGet, base + "/space-1/approval-policy", "spaces:issue"},
		{http.MethodPut, base + "/space-1/approval-policy", "spaces:issue"},
		{http.MethodPost, base + "/tool-gate-view", "spaces:tool-gate:view"},
		{http.MethodPost, base + "/approval-grants/use", "spaces:approval-grant:use"},
		{http.MethodPost, base + "/approval-continuation-check", "spaces:approval-continuation:check"},
	} {
		got := serviceScopeForRequest(httptest.NewRequest(tc.method, tc.path, nil))
		if got != tc.want {
			t.Errorf("%s %s scope = %q, want %q", tc.method, tc.path, got, tc.want)
		}
	}
}

func TestApprovalScopeRulesDoNotMatchOtherMethodsOrShapes(t *testing.T) {
	const base = "/api/v1/internal/spaces"
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, base + "/approval-grants/use"},
		{http.MethodDelete, base + "/approval-grants"},
		{http.MethodPost, base + "/space-1/approval-policy"},
		{http.MethodPatch, base + "/space-1/approval-policy"},
		{http.MethodPost, base + "/space-1/approval-grants/apr_1"},
		{http.MethodGet, base + "/tool-gate-view"},
		{http.MethodGet, base + "/approval-continuation-check"},
		{http.MethodGet, "/api/v1/internal/other/approval-grants"},
	} {
		for _, rule := range spaceFeatureScopeRules {
			if scope, ok := rule(tc.method, tc.path); ok && (scope == spaceToolGateViewScope ||
				scope == spaceApprovalGrantUseScope || scope == spaceApprovalContinuationScope) {
				t.Errorf("%s %s unexpectedly mapped to execution scope %q", tc.method, tc.path, scope)
			}
		}
	}
}

// The three execution-core scopes are plain service scopes: they act on a run,
// not on behalf of a person. spaces:issue stays delegated.
func TestApprovalScopesDelegationRequirements(t *testing.T) {
	if !serviceScopeRequiresVerifiedDelegation("spaces:issue") {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
	for _, scope := range []string{spaceToolGateViewScope, spaceApprovalGrantUseScope, spaceApprovalContinuationScope} {
		if serviceScopeRequiresVerifiedDelegation(scope) {
			t.Errorf("%s is an execution-core service scope and must not demand a user delegation", scope)
		}
	}
	if spaceToolGateViewScope == spaceApprovalGrantUseScope || spaceApprovalGrantUseScope == spaceApprovalContinuationScope ||
		spaceToolGateViewScope == spaceApprovalContinuationScope {
		t.Fatal("each execution-core route needs its own scope")
	}
}

func TestApprovalRoutesAreRegisteredExactlyOnce(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	for _, route := range []string{
		"POST /api/v1/internal/spaces/approval-grants",
		"GET /api/v1/internal/spaces/:space_ref/approval-grants",
		"DELETE /api/v1/internal/spaces/:space_ref/approval-grants/:rule_id",
		"GET /api/v1/internal/spaces/:space_ref/approval-policy",
		"PUT /api/v1/internal/spaces/:space_ref/approval-policy",
		"POST /api/v1/internal/spaces/tool-gate-view",
		"POST /api/v1/internal/spaces/approval-grants/use",
		"POST /api/v1/internal/spaces/approval-continuation-check",
	} {
		if counts[route] != 1 {
			t.Errorf("%s registered %d times, want 1", route, counts[route])
		}
	}
}

// The core Space table is not re-scoped by the approval rules: a core path that
// shares the prefix keeps its own scope.
func TestApprovalRulesDoNotRescopeCoreSpaceRoutes(t *testing.T) {
	for _, tc := range []struct{ method, path, want string }{
		{http.MethodPost, "/api/v1/internal/spaces/register", "spaces:register"},
		{http.MethodPut, "/api/v1/internal/spaces/s1/memberships", "spaces:membership:write"},
		{http.MethodGet, "/api/v1/internal/spaces/s1/membership", "spaces:resolve"},
		{http.MethodPost, "/api/v1/internal/spaces/run-action-decision", "spaces:agent-action:reauthorize"},
	} {
		if got := serviceScopeForRequest(httptest.NewRequest(tc.method, tc.path, nil)); got != tc.want {
			t.Errorf("%s %s scope = %q, want %q", tc.method, tc.path, got, tc.want)
		}
	}
}
