package http

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// WS05-S8: the SCIM edge reaches /internal/scim/v2/* on org:scim:any alone, and the
// org-scoped SCIM administration on /orgs/:id/scim/* needs the signed owner or admin
// scope org:identity:admin:self. The edge principal's own pin is covered in
// scim_handlers_test.go (TestSCIMScopeIsPinnedToTheEdgePrincipal).
func TestSCIMRoutesMapToTheirScopes(t *testing.T) {
	for _, tc := range []struct {
		method string
		path   string
		want   string
	}{
		{http.MethodGet, "/internal/scim/v2/Users", "org:scim:any"},
		{http.MethodPost, "/internal/scim/v2/Users", "org:scim:any"},
		{http.MethodGet, "/orgs/org-1/scim/tokens", "org:identity:admin:self"},
		{http.MethodPost, "/orgs/org-1/scim/tokens", "org:identity:admin:self"},
		{http.MethodDelete, "/orgs/org-1/scim/tokens/token-1", "org:identity:admin:self"},
		{http.MethodGet, "/orgs/org-1/scim/status", "org:identity:admin:self"},
	} {
		got := strings.Join(serviceScopesForRequest(httptest.NewRequest(tc.method, tc.path, nil)), ",")
		if got != tc.want {
			t.Fatalf("%s %s needs scopes %q, want %q", tc.method, tc.path, got, tc.want)
		}
	}
}
