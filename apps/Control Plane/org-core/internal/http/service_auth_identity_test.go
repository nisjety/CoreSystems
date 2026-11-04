package http

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

// WS05-S6/S7: the scope rows of the identity routes and who may hold them.

func TestIdentityServiceScopeForRequest(t *testing.T) {
	admin := []string{"org:identity:admin:self"}
	byDomain := []string{"org:domain:read:any"}
	tests := []struct {
		method string
		path   string
		want   []string
	}{
		// Domain verification (WS05-S6): signed owner/admin through the gateway only.
		{http.MethodGet, "/orgs/org-1/domains", admin},
		{http.MethodPost, "/orgs/org-1/domains", admin},
		{http.MethodPost, "/orgs/org-1/domains/example.com/verify", admin},
		{http.MethodDelete, "/orgs/org-1/domains/example.com", admin},
		{http.MethodPut, "/orgs/org-1/domains", nil},
		{http.MethodPatch, "/orgs/org-1/domains", nil},
		{http.MethodGet, "/orgs/org-1/domains/example.com", nil},
		{http.MethodGet, "/orgs/org-1/domains/example.com/verify", nil},
		{http.MethodPut, "/orgs/org-1/domains/example.com/verify", nil},
		{http.MethodPost, "/orgs/org-1/domains/example.com", nil},

		// SSO policy and providers (WS05-S7): the same authority.
		{http.MethodGet, "/orgs/org-1/sso/policy", admin},
		{http.MethodPut, "/orgs/org-1/sso/policy", admin},
		{http.MethodPost, "/orgs/org-1/sso/policy", nil},
		{http.MethodDelete, "/orgs/org-1/sso/policy", nil},
		{http.MethodPost, "/orgs/org-1/sso/providers", admin},
		{http.MethodDelete, "/orgs/org-1/sso/providers/acme-oidc", admin},
		{http.MethodGet, "/orgs/org-1/sso/providers", nil},
		{http.MethodPut, "/orgs/org-1/sso/providers/acme-oidc", nil},
		{http.MethodGet, "/orgs/org-1/sso/providers/acme-oidc", nil},
		{http.MethodGet, "/orgs/org-1/sso/other", nil},

		// The by-domain reads are machine scopes held by auth-core alone.
		{http.MethodGet, "/internal/orgs/by-domain/example.com", byDomain},
		{http.MethodGet, "/internal/orgs/by-domain/example.com/sso-policy", byDomain},
		{http.MethodPost, "/internal/orgs/by-domain/example.com", nil},
		{http.MethodDelete, "/internal/orgs/by-domain/example.com", nil},
		{http.MethodPut, "/internal/orgs/by-domain/example.com/sso-policy", nil},
		{http.MethodGet, "/internal/orgs/by-domain/example.com/other", nil},
		// Never reachable from the signed-user or /api/v1 prefixes.
		{http.MethodGet, "/orgs/by-domain/example.com", nil},
		{http.MethodGet, "/api/v1/organizations/org-1/domains", nil},
		{http.MethodGet, "/api/v1/organizations/org-1/sso/policy", nil},
	}
	for _, test := range tests {
		request := httptest.NewRequest(test.method, test.path, nil)
		got := serviceScopesForRequest(request)
		if len(got) != len(test.want) {
			t.Fatalf("%s %s scopes=%v want=%v", test.method, test.path, got, test.want)
		}
		for index := range got {
			if got[index] != test.want[index] {
				t.Fatalf("%s %s scopes=%v want=%v", test.method, test.path, got, test.want)
			}
		}
	}
}

// The by-domain path is never read as an organisation id: org:read:any must not reach it.
func TestByDomainNeverFallsThroughToTheOrgRead(t *testing.T) {
	for _, path := range []string{"/internal/orgs/by-domain/example.com", "/internal/orgs/by-domain/example.com/sso-policy"} {
		for _, scope := range serviceScopesForRequest(httptest.NewRequest(http.MethodGet, path, nil)) {
			if scope == "org:read:any" || scope == "org:read:self" {
				t.Fatalf("%s resolved to %s", path, scope)
			}
		}
	}
}

func TestIdentityAdminScopeIsGatewayOnly(t *testing.T) {
	// :self scopes belong to the gateway principal; a machine principal is refused.
	if _, err := parseServiceCredentials(orgServiceTestRegistry("org:identity:admin:self")); err != nil {
		t.Fatalf("gateway with org:identity:admin:self rejected: %v", err)
	}
	for _, principal := range []string{"auth-core", "billing-core", "machine-a"} {
		raw := `[{"principal":"` + principal + `","audience":"org-core","token":"fedcba9876543210fedcba9876543210","scopes":["org:identity:admin:self"]}]`
		if _, err := parseServiceCredentials(raw); err == nil {
			t.Fatalf("principal %s must not hold org:identity:admin:self", principal)
		}
	}
}

func TestDomainReadScopeIsAuthCoreOnly(t *testing.T) {
	auth := `[{"principal":"auth-core","audience":"org-core","token":"fedcba9876543210fedcba9876543210","scopes":["org:domain:read:any"]}]`
	if credentials, err := parseServiceCredentials(auth); err != nil || len(credentials) != 1 {
		t.Fatalf("auth-core with org:domain:read:any rejected: credentials=%d err=%v", len(credentials), err)
	}
	// The :any suffix keeps it off the gateway.
	if _, err := parseServiceCredentials(orgServiceTestRegistry("org:domain:read:any")); err == nil {
		t.Fatal("the gateway must not hold org:domain:read:any")
	}
	// And every other machine principal is refused.
	for _, principal := range []string{"billing-core", "session-core", "audit-core", "verevon-scim-edge", "machine-a"} {
		raw := `[{"principal":"` + principal + `","audience":"org-core","token":"fedcba9876543210fedcba9876543210","scopes":["org:domain:read:any"]}]`
		if _, err := parseServiceCredentials(raw); err == nil {
			t.Fatalf("principal %s must not hold org:domain:read:any", principal)
		}
	}
	// auth-core may hold it next to its other reads.
	both := `[{"principal":"auth-core","audience":"org-core","token":"fedcba9876543210fedcba9876543210","scopes":["org:read:any","org:retention:read:any","org:domain:read:any"]}]`
	if _, err := parseServiceCredentials(both); err != nil {
		t.Fatalf("auth-core with its full read set rejected: %v", err)
	}
}
