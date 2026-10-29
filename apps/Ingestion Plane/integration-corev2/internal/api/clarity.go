package api

import (
	"bytes"
	"net/url"
	"strconv"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

func registerClarity(app *fiber.App, cfg ServerConfig, authenticate fiber.Handler, limits []fiber.Handler) {
	handlers := append(append([]fiber.Handler{}, limits...), authenticate, pageSpeedReadScope)
	app.Get("/api/v1/insights/clarity", append(handlers, func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		if cfg.Clarity == nil {
			return apiError(c, 503, "clarity_unavailable", "Clarity reporting is unavailable.")
		}
		query, err := url.ParseQuery(string(c.Context().QueryArgs().QueryString()))
		if err != nil || len(query) != 2 || len(query["site"]) != 1 || len(query["numOfDays"]) != 1 || len(bytes.TrimSpace(c.Body())) != 0 {
			return invalidClarity(c)
		}
		site := query.Get("site")
		days, err := strconv.Atoi(query.Get("numOfDays"))
		if err != nil || strconv.Itoa(days) != query.Get("numOfDays") || days < 1 || days > 3 || pagespeed.ValidateInput(site, "mobile") != nil {
			return invalidClarity(c)
		}
		principal, _ := auth.PrincipalFromContext(c)
		return success(c, cfg.Clarity.Get(c.UserContext(), strings.Clone(principal.OrganizationID), strings.Clone(site), days))
	})...)
	app.Get("/api/v1/insights/clarity/binding", append(handlers, func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		if cfg.Clarity == nil {
			return apiError(c, 503, "clarity_unavailable", "Clarity reporting is unavailable.")
		}
		query, err := url.ParseQuery(string(c.Context().QueryArgs().QueryString()))
		if err != nil || len(query) != 1 || len(query["site"]) != 1 || len(bytes.TrimSpace(c.Body())) != 0 || pagespeed.ValidateInput(query.Get("site"), "mobile") != nil {
			return apiError(c, 400, "invalid_clarity_request", "Choose one canonical public HTTPS website origin.")
		}
		principal, _ := auth.PrincipalFromContext(c)
		access, err := cfg.Clarity.Access(strings.Clone(principal.OrganizationID), strings.Clone(query.Get("site")))
		if err != nil {
			return apiError(c, 503, "clarity_unavailable", "Clarity reporting is unavailable.")
		}
		return success(c, access)
	})...)
	app.All("/api/v1/insights/clarity/binding", append(handlers, func(c *fiber.Ctx) error {
		c.Set("Allow", "GET")
		c.Set("Cache-Control", "no-store")
		return apiError(c, 405, "method_not_allowed", "This endpoint is read-only.")
	})...)
}

func invalidClarity(c *fiber.Ctx) error {
	return apiError(c, 400, "invalid_clarity_request", "Choose one canonical public HTTPS website origin and a 1, 2, or 3 day Clarity window.")
}
