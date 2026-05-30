package middleware

import (
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

type MonitoringConfig struct {
	Enabled bool
}

func JobMonitoring(cfg MonitoringConfig) fiber.Handler {
	if !cfg.Enabled {
		return func(c *fiber.Ctx) error { return c.Next() }
	}

	return func(c *fiber.Ctx) error {
		path := c.Path()
		if !strings.HasPrefix(path, "/v1/") {
			return c.Next()
		}

		start := time.Now()
		err := c.Next()
		duration := time.Since(start)

		jobID := c.Params("id")
		event := "request_completed"
		if strings.Contains(path, "/crawl") || strings.Contains(path, "/batch") || strings.Contains(path, "/jobs") {
			event = "job_signal"
		}

		logger := log.Info().
			Str("event", event).
			Str("method", c.Method()).
			Str("path", path).
			Int("status", c.Response().StatusCode()).
			Dur("duration", duration).
			Str("request_id", c.GetRespHeader("X-Request-ID"))

		if jobID != "" {
			logger = logger.Str("job_id", jobID)
		}

		logger.Msg("request monitored")
		return err
	}
}
