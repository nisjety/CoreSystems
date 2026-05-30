package api

import (
	"crypto/subtle"
	"strings"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"
)

func APIKeyMiddleware(headerName, expectedAPIKey string) fiber.Handler {
	if headerName == "" {
		headerName = "X-API-Key"
	}
	return func(c *fiber.Ctx) error {
		provided := strings.TrimSpace(c.Get(headerName))
		expected := strings.TrimSpace(expectedAPIKey)
		if provided == "" || subtle.ConstantTimeCompare([]byte(provided), []byte(expected)) != 1 {
			log.Warn().
				Str("event", "auth_denied").
				Str("path", c.Path()).
				Str("ip", c.IP()).
				Str("request_id", c.GetRespHeader("X-Request-ID")).
				Msg("API key validation failed")
			return writeError(c, fiber.StatusUnauthorized, "invalid or missing API key", nil)
		}
		log.Info().
			Str("event", "auth_granted").
			Str("path", c.Path()).
			Str("ip", c.IP()).
			Str("request_id", c.GetRespHeader("X-Request-ID")).
			Msg("API key validated")
		return c.Next()
	}
}
