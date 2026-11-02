package http

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestRegisterSpaceScopeRuleCannotRescopeCorePath(t *testing.T) {
	saved := spaceFeatureScopeRules
	t.Cleanup(func() { spaceFeatureScopeRules = saved })

	// A hostile or careless feature rule that tries to widen every spaces path
	// onto a weak scope, plus a rule for a path nobody owns yet.
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if len(path) >= len("/api/v1/internal/spaces") && path[:len("/api/v1/internal/spaces")] == "/api/v1/internal/spaces" {
			return "feature:widened", true
		}
		return "", false
	})

	core := []struct{ method, path, want string }{
		{http.MethodPost, "/api/v1/internal/spaces/register", "spaces:register"},
		{http.MethodPut, "/api/v1/internal/spaces/s1/memberships", "spaces:membership:write"},
		{http.MethodGet, "/api/v1/internal/spaces", "spaces:resolve"},
		{http.MethodPost, "/api/v1/internal/spaces/thread-append-decision", "spaces:issue"},
		{http.MethodPut, "/api/v1/internal/spaces/effect-policy", "spaces:policy:write"},
		{http.MethodPost, "/api/v1/internal/spaces/run-action-decision", "spaces:agent-action:reauthorize"},
	}
	for _, tc := range core {
		got := serviceScopeForRequest(httptest.NewRequest(tc.method, tc.path, nil))
		if got != tc.want {
			t.Errorf("%s %s re-scoped by a feature rule: got %q, want %q", tc.method, tc.path, got, tc.want)
		}
	}

	// A path the core table does not know falls through to the feature rule.
	got := serviceScopeForRequest(httptest.NewRequest(http.MethodGet, "/api/v1/internal/spaces/s1/p-uc1-new-thing", nil))
	if got != "feature:widened" {
		t.Errorf("unmapped spaces path should reach feature rules, got %q", got)
	}
}

func TestFeatureRuleForUnknownPathDoesNotAffectNonSpacePaths(t *testing.T) {
	saved := spaceFeatureScopeRules
	t.Cleanup(func() { spaceFeatureScopeRules = saved })
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if path == "/api/v1/internal/spaces/p-uc1-only" {
			return "spaces:p-uc1", true
		}
		return "", false
	})
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces/p-uc1-only", nil)); got != "spaces:p-uc1" {
		t.Fatalf("feature rule not consulted: %q", got)
	}
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, "/api/v1/internal/authz/grant", nil)); got != "authz:write" {
		t.Fatalf("non-space path scope changed: %q", got)
	}
}

func TestRegisterSpaceDelegatedScopePrefixRequiresVerifiedDelegation(t *testing.T) {
	saved := spaceDelegatedScopePrefixes
	t.Cleanup(func() { spaceDelegatedScopePrefixes = saved })

	const scope = "p-uc1-feature:read"
	if spaceScopeRequiresVerifiedDelegation(scope) || serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatalf("scope %q must not require delegation before its prefix is registered", scope)
	}

	registerSpaceDelegatedScopePrefix("p-uc1-feature:")
	if !spaceScopeRequiresVerifiedDelegation(scope) {
		t.Fatalf("prefixed scope %q must require verified delegation", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatalf("serviceScopeRequiresVerifiedDelegation must honour the registered prefix for %q", scope)
	}
	// A near-miss prefix does not match.
	if spaceScopeRequiresVerifiedDelegation("p-uc1-featurex:read") {
		t.Fatal("prefix match must be literal")
	}
}

func TestCoreSpaceDelegatedScopesAreAlwaysDelegated(t *testing.T) {
	for _, scope := range []string{"spaces:resolve", "spaces:issue"} {
		if !spaceScopeRequiresVerifiedDelegation(scope) {
			t.Errorf("core scope %q must require verified delegation", scope)
		}
	}
	for _, scope := range []string{"spaces:register", "spaces:membership:write", "spaces:policy:write"} {
		if spaceScopeRequiresVerifiedDelegation(scope) {
			t.Errorf("service-only scope %q must not require delegation", scope)
		}
	}
}
