package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/pagespeed"
	"github.com/triodelab/integration-corev2/internal/publicsignals"
)

type PublicSignalsReader interface {
	Collect(context.Context, string) (publicsignals.Report, error)
}

func registerPublicSignals(app *fiber.App, cfg ServerConfig, authenticate fiber.Handler, limits []fiber.Handler) {
	handlers := append(append([]fiber.Handler{}, limits...), authenticate, pageSpeedReadScope)
	app.Post("/api/v1/insights/public-signals", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		if cfg.PublicSignals == nil {
			return apiError(c, 503, "public_signals_unavailable", "Public web signals are unavailable.")
		}
		media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
		if err != nil || media != "application/json" || len(c.Context().QueryArgs().QueryString()) != 0 {
			return invalidPublicSignals(c)
		}
		site, err := parsePublicSignalsBody(c.Body())
		if err != nil || pagespeed.ValidateInput(site, "mobile") != nil {
			return invalidPublicSignals(c)
		}
		report, err := cfg.PublicSignals.Collect(c.UserContext(), strings.Clone(site))
		if errors.Is(err, publicsignals.ErrBusy) {
			c.Set("Retry-After", "60")
			return apiError(c, 429, "public_signals_busy", "Public web signals capacity is busy. Retry later.")
		}
		if err != nil {
			return apiError(c, 503, "public_signals_unavailable", "Public web signals are unavailable.")
		}
		return success(c, report)
	})...)
	app.All("/api/v1/insights/public-signals", append(append([]fiber.Handler{}, handlers...), func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		c.Set("Allow", "POST")
		return apiError(c, 405, "method_not_allowed", "Use a read-only public signals request.")
	})...)
}
func invalidPublicSignals(c *fiber.Ctx) error {
	return apiError(c, 400, "invalid_public_signals_request", "Choose one canonical public HTTPS website origin.")
}
func parsePublicSignalsBody(body []byte) (string, error) {
	if len(body) > 2048 {
		return "", publicsignals.ErrInvalid
	}
	d := json.NewDecoder(bytes.NewReader(body))
	token, err := d.Token()
	if err != nil || token != json.Delim('{') {
		return "", publicsignals.ErrInvalid
	}
	key, err := d.Token()
	if err != nil || key != "site" {
		return "", publicsignals.ErrInvalid
	}
	var site string
	if d.Decode(&site) != nil || site == "" || d.More() {
		return "", publicsignals.ErrInvalid
	}
	end, err := d.Token()
	if err != nil || end != json.Delim('}') || d.Decode(new(any)) != io.EOF {
		return "", publicsignals.ErrInvalid
	}
	return site, nil
}
