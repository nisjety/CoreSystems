package api

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/trends"
	"github.com/triodelab/integration-corev2/internal/webvitals"
	"io"
	"mime"
)

type TrendsReader interface {
	Collect(context.Context, trends.Query) (trends.Report, error)
}

func registerTrends(app *fiber.App, cfg ServerConfig, authenticate fiber.Handler, limits []fiber.Handler) {
	handlers := append(append([]fiber.Handler{}, limits...), authenticate, pageSpeedReadScope)
	app.Post("/api/v1/insights/trends", append(handlers, func(c *fiber.Ctx) error {
		c.Set("Cache-Control", "no-store")
		media, _, err := mime.ParseMediaType(c.Get("Content-Type"))
		var q trends.Query
		d := json.NewDecoder(bytes.NewReader(c.Body()))
		d.DisallowUnknownFields()
		if err != nil || media != "application/json" || len(c.Context().QueryArgs().QueryString()) != 0 || len(c.Body()) > 2048 || webvitals.UniqueJSON(c.Body()) != nil || d.Decode(&q) != nil || d.Decode(new(any)) != io.EOF || !trends.Valid(q) {
			return apiError(c, 400, "invalid_trends_request", "Choose up to five search terms, a country and a supported period.")
		}
		if cfg.Trends == nil {
			return apiError(c, 503, "trends_unavailable", "Development Trends reporting is unavailable.")
		}
		report, err := cfg.Trends.Collect(c.UserContext(), q)
		if err != nil {
			return apiError(c, 503, "trends_unavailable", "Development Trends reporting is unavailable.")
		}
		return success(c, report)
	})...)
}
