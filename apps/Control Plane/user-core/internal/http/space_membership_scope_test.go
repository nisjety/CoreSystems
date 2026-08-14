package http

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestSpaceMembershipScopeRouting(t *testing.T) {
	cases := []struct{ method, path, want string }{
		{http.MethodPut, "/api/v1/internal/spaces/space-1/memberships", "spaces:membership:write"},
		{http.MethodGet, "/api/v1/internal/spaces/space-1/membership", "spaces:resolve"},
		{http.MethodPost, "/api/v1/internal/spaces/register", "spaces:register"},
		{http.MethodPost, "/api/v1/internal/spaces/recipient-audiences", "spaces:audience:publish"},
	}
	for _, tc := range cases {
		got := serviceScopeForRequest(httptest.NewRequest(tc.method, tc.path, nil))
		if got != tc.want {
			t.Errorf("%s %s => %q, want %q", tc.method, tc.path, got, tc.want)
		}
	}
	// A membership WRITE must never resolve to the read scope: they are
	// separate powers and the read scope is far more widely held.
	if serviceScopeForRequest(httptest.NewRequest(http.MethodPut, "/api/v1/internal/spaces/s/memberships", nil)) == "spaces:resolve" {
		t.Fatal("membership write collapsed onto the read scope")
	}
}
