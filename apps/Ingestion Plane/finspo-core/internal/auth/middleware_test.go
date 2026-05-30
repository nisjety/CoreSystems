package auth

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gofiber/fiber/v2"
)

func newTestApp(cfg Config) *fiber.App {
	app := fiber.New()
	app.Use(Middleware(cfg))
	app.Get("/protected", func(c *fiber.Ctx) error {
		orgID, _ := c.Locals("org_id").(string)
		if orgID == "" {
			return c.Status(http.StatusInternalServerError).SendString("missing org context")
		}
		return c.SendStatus(http.StatusNoContent)
	})
	return app
}

func TestMiddlewareRejectsMissingKey(t *testing.T) {
	t.Parallel()

	app := newTestApp(Config{APIKey: "test-key"})
	resp, err := app.Test(httptest.NewRequest(http.MethodGet, "/protected", nil), -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestMiddlewareAcceptsPrimaryHeader(t *testing.T) {
	t.Parallel()

	app := newTestApp(Config{APIKey: "test-key", APIKeyHeader: "X-API-Key"})
	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("X-API-Key", "test-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNoContent {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusNoContent)
	}
}

func TestMiddlewareAcceptsInternalHeaderFallback(t *testing.T) {
	t.Parallel()

	app := newTestApp(Config{APIKey: "test-key"})
	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNoContent {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusNoContent)
	}
}

func TestMiddlewareRejectsInvalidKey(t *testing.T) {
	t.Parallel()

	app := newTestApp(Config{APIKey: "test-key"})
	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("X-API-Key", "wrong-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestMiddlewareRejectsWhenConfiguredKeyIsEmpty(t *testing.T) {
	t.Parallel()

	app := newTestApp(Config{})
	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("X-API-Key", "anything")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestMiddlewareRejectsMissingOrgHeader(t *testing.T) {
	t.Parallel()

	app := newTestApp(Config{APIKey: "test-key"})
	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("X-API-Key", "test-key")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}
