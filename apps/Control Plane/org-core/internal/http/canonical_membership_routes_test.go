package http

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestLegacyDirectMembershipAndRoleMutationsAreNotMounted(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "test-internal-key")
	t.Setenv("INTERNAL_SERVICE_SECRET", "")
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
			response := httptest.NewRecorder()
			request := httptest.NewRequest(test.method, test.path, strings.NewReader(test.body))
			request.Header.Set("Content-Type", "application/json")
			request.Header.Set("X-Internal-Api-Key", "test-internal-key")
			request.Header.Set("X-User-Id", "ordinary-member")
			server.router.ServeHTTP(response, request)
			if response.Code != http.StatusNotFound {
				t.Fatalf("status = %d; want 404 so Auth Core remains the only membership mutation authority", response.Code)
			}
		})
	}
}

func TestAuthProjectionMembershipRouteRemainsMounted(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "test-internal-key")
	t.Setenv("INTERNAL_SERVICE_SECRET", "")
	server := NewServer(0, nil, nil, "", "")
	response := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodPost, "/internal/orgs/org-1/members/reconcile", strings.NewReader(`{"userId":"user-1","role":"member","action":"upsert","revision":1}`))
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Internal-Api-Key", "test-internal-key")
	server.router.ServeHTTP(response, request)
	if response.Code == http.StatusNotFound {
		t.Fatal("canonical Auth projection route must remain mounted")
	}
}
