package api

import (
	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
)

// requireServiceIntegrationWrite constrains service callers without replacing
// the interactive principal and organization checks in each route handler.
func requireServiceIntegrationWrite(c *fiber.Ctx) error {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok {
		return apiError(c, fiber.StatusUnauthorized, "unauthorized", "Authentication required")
	}
	if principal.PrincipalType == "service" && !principal.HasScope("integration:write") {
		return apiError(c, fiber.StatusForbidden, "integration_write_scope_required", "Service integration mutations require integration:write")
	}
	return c.Next()
}
