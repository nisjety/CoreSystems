package auth

import (
	"crypto/subtle"
	"strings"

	"github.com/gofiber/fiber/v2"
)

const (
	OrganizationIDHeader = "X-Org-ID"
	organizationIDLocal  = "org_id"
)

type Config struct {
	APIKey       string
	APIKeyHeader string
}

func Middleware(cfg Config) fiber.Handler {
	header := strings.TrimSpace(cfg.APIKeyHeader)
	if header == "" {
		header = "X-API-Key"
	}

	return func(c *fiber.Ctx) error {
		provided := strings.TrimSpace(c.Get(header))
		if provided == "" {
			provided = strings.TrimSpace(c.Get("x-internal-api-key"))
		}

		if provided == "" || cfg.APIKey == "" {
			return writeError(c, fiber.StatusUnauthorized, "authentication required")
		}

		if subtle.ConstantTimeCompare([]byte(provided), []byte(cfg.APIKey)) != 1 {
			return writeError(c, fiber.StatusUnauthorized, "invalid API key")
		}

		organizationID := strings.TrimSpace(c.Get(OrganizationIDHeader))
		if organizationID == "" {
			return writeError(c, fiber.StatusBadRequest, "organization context is required")
		}

		c.Locals(organizationIDLocal, organizationID)

		return c.Next()
	}
}

func OrganizationID(c *fiber.Ctx) string {
	organizationID, _ := c.Locals(organizationIDLocal).(string)
	return strings.TrimSpace(organizationID)
}

func writeError(c *fiber.Ctx, status int, message string) error {
	return c.Status(status).JSON(fiber.Map{
		"success": false,
		"error":   message,
	})
}
