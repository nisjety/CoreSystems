package http

import "testing"

func TestInternalOrganizationReadRouteIsMounted(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")
	for _, route := range server.router.Routes() {
		if route.Method == "GET" && route.Path == "/internal/orgs/:orgId" {
			return
		}
	}
	t.Fatal("GET /internal/orgs/:orgId is not mounted")
}
