package api

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/url"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

func registerPageSpeed(app *fiber.App, cfg ServerConfig, authenticate fiber.Handler, limits []fiber.Handler) {
	handlers := append(append([]fiber.Handler{}, limits...), authenticate, pageSpeedReadScope)
	app.Get("/api/v1/insights/pagespeed", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		if cfg.PageSpeed == nil {
			return apiError(c, 503, "pagespeed_unavailable", "PageSpeed diagnostics are unavailable.")
		}
		query, err := url.ParseQuery(string(c.Context().QueryArgs().QueryString()))
		if err != nil || len(query) != 2 || len(query["site"]) != 1 || len(query["strategy"]) != 1 || len(bytes.TrimSpace(c.Body())) != 0 {
			return invalidPageSpeed(c)
		}
		site, strategy := query.Get("site"), query.Get("strategy")
		if pagespeed.ValidateInput(site, strategy) != nil {
			return invalidPageSpeed(c)
		}
		principal, _ := auth.PrincipalFromContext(c)
		return success(c, cfg.PageSpeed.Get(principal.OrganizationID, site, strategy))
	})...)
	app.Post("/api/v1/insights/pagespeed", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		if cfg.PageSpeed == nil {
			return apiError(c, 503, "pagespeed_unavailable", "PageSpeed diagnostics are unavailable.")
		}
		media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
		if err != nil || media != "application/json" || len(c.Context().QueryArgs().QueryString()) > 0 {
			return invalidPageSpeed(c)
		}
		site, strategy, err := parsePageSpeedBody(c.Body())
		if err != nil || pagespeed.ValidateInput(site, strategy) != nil {
			return invalidPageSpeed(c)
		}
		principal, _ := auth.PrincipalFromContext(c)
		snapshot, err := cfg.PageSpeed.Start(strings.Clone(principal.OrganizationID), strings.Clone(site), strings.Clone(strategy))
		if errors.Is(err, pagespeed.ErrBusy) {
			c.Set("Retry-After", "60")
			return apiError(c, 429, "pagespeed_busy", "PageSpeed audit capacity is busy. Try again later.")
		}
		if err != nil {
			return apiError(c, 503, "pagespeed_unavailable", "PageSpeed diagnostics are unavailable.")
		}
		if snapshot.Status == "running" {
			c.Status(fiber.StatusAccepted)
		}
		return success(c, snapshot)
	})...)
}

func pageSpeedReadScope(c *fiber.Ctx) error {
	principal, ok := auth.PrincipalFromContext(c)
	if !ok || auth.IsInternalCall(c) || strings.TrimSpace(principal.OrganizationID) == "" {
		return apiError(c, 403, "organization_required", "Verified organization context is required.")
	}
	if principal.PrincipalType == "service" && !principal.HasScope("integration:read") {
		return apiError(c, 403, "integration_read_scope_required", "Read-only provider diagnostics require integration:read.")
	}
	return c.Next()
}

func invalidPageSpeed(c *fiber.Ctx) error {
	return apiError(c, 400, "invalid_pagespeed_request", "Choose one canonical public HTTPS website origin and mobile or desktop strategy.")
}

func parsePageSpeedBody(body []byte) (string, string, error) {
	invalid := errors.New("invalid audit input")
	if len(body) > 2048 {
		return "", "", invalid
	}
	decoder := json.NewDecoder(bytes.NewReader(body))
	token, err := decoder.Token()
	if err != nil || token != json.Delim('{') {
		return "", "", invalid
	}
	values := map[string]string{}
	for decoder.More() {
		key, err := decoder.Token()
		if err != nil {
			return "", "", invalid
		}
		name, ok := key.(string)
		if !ok || name != "site" && name != "strategy" {
			return "", "", invalid
		}
		if _, duplicate := values[name]; duplicate {
			return "", "", invalid
		}
		var value string
		if decoder.Decode(&value) != nil {
			return "", "", invalid
		}
		values[name] = value
	}
	if _, err := decoder.Token(); err != nil || len(values) != 2 {
		return "", "", invalid
	}
	if _, err := decoder.Token(); err != io.EOF {
		return "", "", invalid
	}
	return values["site"], values["strategy"], nil
}
