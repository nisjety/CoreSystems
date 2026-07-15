package http

import (
	"net/http"
	"testing"
)

func TestLegacyDirectMembershipAndRoleMutationsAreNotMounted(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")

	tests := []struct {
		method string
		path   string
		body   string
	}{
		{http.MethodPost, "/orgs/org-1/members/invite", `{"email":"victim@example.com","role":"owner"}`},
		{http.MethodDelete, "/orgs/org-1/members/user-1", ""},
		{http.MethodPatch, "/orgs/org-1/members/user-1/role", `{"role":"owner"}`},
		{http.MethodPost, "/orgs/org-1/roles", `{"role_name":"owner-copy","permissions":["*"]}`},
		{http.MethodPatch, "/orgs/org-1/roles/custom", `{"permissions":["*"]}`},
		{http.MethodDelete, "/orgs/org-1/roles/custom", ""},
	}

	for _, test := range tests {
		t.Run(test.method+" "+test.path, func(t *testing.T) {
			for _, route := range server.router.Routes() {
				if route.Method == test.method && route.Path == test.path {
					t.Fatalf("route %s %s is mounted; Auth Core must remain the only membership mutation authority", test.method, test.path)
				}
			}
		})
	}
}

func TestAuthProjectionMembershipRouteRemainsMounted(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")
	for _, route := range server.router.Routes() {
		if route.Method == http.MethodPost && route.Path == "/internal/orgs/:orgId/members/reconcile" {
			return
		}
	}
	t.Fatal("canonical Auth projection route is not mounted")
}
