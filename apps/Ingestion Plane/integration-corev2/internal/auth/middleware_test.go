package auth

import (
	"context"
	"net/http/httptest"
	"testing"

	"github.com/gofiber/fiber/v2"
)

func TestInternalOrBearerRejectsSharedKeyForTenantRoutesByDefault(t *testing.T) {
	app := fiber.New()
	app.Get("/tenant", InternalOrBearer(Config{APIKey: "synthetic-shared-key"}), func(c *fiber.Ctx) error {
		return c.SendStatus(fiber.StatusNoContent)
	})
	request := httptest.NewRequest("GET", "/tenant", nil)
	request.Header.Set("X-Internal-API-Key", "synthetic-shared-key")
	request.Header.Set("X-Org-ID", "org-forged")

	response, err := app.Test(request)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	if response.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.StatusCode, fiber.StatusForbidden)
	}
}

func TestInternalOrBearerAcceptsVerifiedTenantPrincipal(t *testing.T) {
	app := fiber.New()
	app.Get("/tenant", InternalOrBearer(Config{
		APIKey: "synthetic-shared-key",
		TokenVerifier: staticVerifier{principal: Principal{
			UserID:         "user-test",
			OrganizationID: "org-test",
		}},
	}), func(c *fiber.Ctx) error {
		principal, ok := PrincipalFromContext(c)
		if !ok || principal.OrganizationID != "org-test" {
			return c.SendStatus(fiber.StatusInternalServerError)
		}
		return c.SendStatus(fiber.StatusNoContent)
	})
	request := httptest.NewRequest("GET", "/tenant", nil)
	request.Header.Set("Authorization", "Bearer verified-token")

	response, err := app.Test(request)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	if response.StatusCode != fiber.StatusNoContent {
		t.Fatalf("status = %d, want %d", response.StatusCode, fiber.StatusNoContent)
	}
}

type staticVerifier struct {
	principal Principal
}

func (v staticVerifier) VerifyToken(context.Context, string) (Principal, error) {
	return v.principal, nil
}
