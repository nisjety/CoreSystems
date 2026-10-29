package api

import (
	"context"
	"errors"
	"mime"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/webvitals"
)

type WebVitalsReader interface {
	Get(context.Context, string, webvitals.Input) (webvitals.Report, error)
}

func registerWebVitals(app *fiber.App, cfg ServerConfig, authenticate fiber.Handler, limits []fiber.Handler) {
	handlers := append(append([]fiber.Handler{}, limits...), authenticate, pageSpeedReadScope)
	app.Post("/api/v1/insights/web-vitals/binding", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
		if err != nil || media != "application/json" || len(c.Context().QueryArgs().QueryString()) != 0 {
			return invalidWebVitals(c)
		}
		input, err := webvitals.ParseInput(c.Body())
		if err != nil {
			return invalidWebVitals(c)
		}
		reader, ok := cfg.WebVitals.(interface {
			Configured(string, webvitals.Input) (bool, error)
		})
		if !ok {
			return apiError(c, 503, "web_vitals_unavailable", "First-party performance setup is unavailable.")
		}
		principal, _ := auth.PrincipalFromContext(c)
		configured, err := reader.Configured(strings.Clone(principal.OrganizationID), input)
		if err != nil {
			return apiError(c, 503, "web_vitals_unavailable", "First-party performance setup is unavailable.")
		}
		return success(c, fiber.Map{"configured": configured})
	})...)
	app.Post("/api/v1/insights/web-vitals", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
		if err != nil || media != "application/json" || len(c.Context().QueryArgs().QueryString()) != 0 {
			return invalidWebVitals(c)
		}
		input, err := webvitals.ParseInput(c.Body())
		if err != nil {
			return invalidWebVitals(c)
		}
		if cfg.WebVitals == nil {
			return apiError(c, 503, "web_vitals_unavailable", "First-party performance reporting is unavailable.")
		}
		principal, _ := auth.PrincipalFromContext(c)
		report, err := cfg.WebVitals.Get(c.UserContext(), strings.Clone(principal.OrganizationID), input)
		if errors.Is(err, webvitals.ErrBusy) {
			c.Set("Retry-After", "60")
			return apiError(c, 429, "web_vitals_busy", "First-party performance capacity is busy. Retry later.")
		}
		if err != nil {
			return apiError(c, 503, "web_vitals_unavailable", "First-party performance reporting is unavailable.")
		}
		// This bounded machine receipt deliberately has only the two agreed
		// fields. The request ID remains available in the response header.
		return c.JSON(fiber.Map{"success": true, "data": report})
	})...)
	app.All("/api/v1/insights/web-vitals", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Allow", "POST")
		c.Set("Cache-Control", "no-store")
		return apiError(c, 405, "method_not_allowed", "Use a read-only first-party performance request.")
	})...)
}
func invalidWebVitals(c *fiber.Ctx) error {
	return apiError(c, 400, "invalid_web_vitals_request", "Choose one registered website, current revision and performance environment.")
}
