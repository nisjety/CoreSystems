package api

import (
	"context"
	"errors"
	"mime"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/sitesources"
)

type SiteSourcesReader interface {
	Collect(context.Context, string, sitesources.Input) (sitesources.Report, error)
}

func siteSourceReadScope(c *fiber.Ctx) error {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) || strings.TrimSpace(principal.OrganizationID) == "" {
		return apiError(c, 403, "organization_required", "Verified organization context is required.")
	}
	if !principal.HasScope("integration:read") {
		return apiError(c, 403, "integration_read_scope_required", "Read-only site sources require integration:read.")
	}
	return c.Next()
}
func registerSiteSources(app *fiber.App, cfg ServerConfig, authenticate fiber.Handler, limits []fiber.Handler) {
	handlers := append(append([]fiber.Handler{}, limits...), authenticate, siteSourceReadScope)
	app.Post("/api/v1/insights/site-sources/binding", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
		if err != nil || media != "application/json" || len(c.Context().QueryArgs().QueryString()) != 0 {
			return invalidSiteSources(c)
		}
		input, err := sitesources.ParseInput(c.Body())
		if err != nil {
			return invalidSiteSources(c)
		}
		reader, ok := cfg.SiteSources.(interface {
			Configured(string, sitesources.Input) (bool, error)
		})
		if !ok {
			return apiError(c, 503, "site_sources_unavailable", "Site source setup is unavailable.")
		}
		principal, _ := auth.PrincipalFromContext(c)
		configured, err := reader.Configured(strings.Clone(principal.OrganizationID), input)
		if err != nil {
			return apiError(c, 503, "site_sources_unavailable", "Site source setup is unavailable.")
		}
		return success(c, fiber.Map{"configured": configured})
	})...)
	app.Post("/api/v1/insights/site-sources", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
		if err != nil || media != "application/json" || len(c.Context().QueryArgs().QueryString()) != 0 {
			return invalidSiteSources(c)
		}
		input, err := sitesources.ParseInput(c.Body())
		if err != nil {
			return invalidSiteSources(c)
		}
		if cfg.SiteSources == nil {
			return apiError(c, 503, "site_sources_unavailable", "Site source reporting is unavailable.")
		}
		principal, _ := auth.PrincipalFromContext(c)
		report, err := cfg.SiteSources.Collect(c.UserContext(), strings.Clone(principal.OrganizationID), input)
		if errors.Is(err, sitesources.ErrBusy) {
			c.Set("Retry-After", "60")
			return apiError(c, 429, "site_sources_busy", "Site source reporting capacity is busy. Retry later.")
		}
		if err != nil {
			return apiError(c, 503, "site_sources_unavailable", "Site source reporting is unavailable.")
		}
		return c.JSON(fiber.Map{"success": true, "data": report})
	})...)
	app.All("/api/v1/insights/site-sources", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Allow", "POST")
		c.Set("Cache-Control", "no-store")
		return apiError(c, 405, "method_not_allowed", "Use a read-only site source request.")
	})...)
}
func invalidSiteSources(c *fiber.Ctx) error {
	return apiError(c, 400, "invalid_site_sources_request", "Choose a registered website, current revision and supported site provider.")
}
