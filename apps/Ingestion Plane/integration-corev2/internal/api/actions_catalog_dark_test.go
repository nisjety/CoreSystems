package api

import (
	"net/http/httptest"
	"testing"

	"github.com/gofiber/fiber/v2"
)

// The provider operation catalog is dark while INTEGRATION_ACTIONS_CATALOG is off: its route is
// not registered, so GET /api/v1/actions/catalog answers 404 like any unknown path, with or
// without the internal key (a registered route would refuse the keyless request with 401). With
// the switch on, the internal key reads the catalog.

// actionsCatalogStatus answers GET /api/v1/actions/catalog from a server built with the given
// switch value, sending the internal key only when internalKey is true.
func actionsCatalogStatus(t *testing.T, catalogEnabled, internalKey bool) int {
	t.Helper()
	cfg, repo, oauthService := testOAuthStack(t)
	cfg.ActionsCatalogEnabled = catalogEnabled
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService})
	req := httptest.NewRequest("GET", "/api/v1/actions/catalog", nil)
	if internalKey {
		req.Header.Set("X-Internal-API-Key", "dev-key")
	}
	response, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	defer response.Body.Close()
	return response.StatusCode
}

// While the switch is off the route is absent: 404 with or without the internal key.
func TestActionsCatalogRouteIsAbsentWhileDark(t *testing.T) {
	for _, internalKey := range []bool{true, false} {
		if status := actionsCatalogStatus(t, false, internalKey); status != fiber.StatusNotFound {
			t.Fatalf("catalog status = %d (internal key sent: %v) while INTEGRATION_ACTIONS_CATALOG is off, want 404", status, internalKey)
		}
	}
}

// With the switch on, the internal key reads the catalog.
func TestActionsCatalogRouteIsServedWhileLive(t *testing.T) {
	if status := actionsCatalogStatus(t, true, true); status != fiber.StatusOK {
		t.Fatalf("catalog status = %d while INTEGRATION_ACTIONS_CATALOG is on, want 200", status)
	}
}
