package middleware

import (
	"context"
	"time"

	"github.com/gofiber/fiber/v2"
)

type RetryConfig struct {
	Enabled     bool
	MaxRetries  int
	BaseBackoff time.Duration
}

func CustomRetry(cfg RetryConfig) fiber.Handler {
	if !cfg.Enabled || cfg.MaxRetries <= 0 {
		return func(c *fiber.Ctx) error { return c.Next() }
	}

	baseBackoff := cfg.BaseBackoff
	if baseBackoff <= 0 {
		baseBackoff = 250 * time.Millisecond
	}

	return func(c *fiber.Ctx) error {
		method := c.Method()
		if method != fiber.MethodGet && method != fiber.MethodHead && method != fiber.MethodOptions {
			return c.Next()
		}

		attempt, _ := c.Locals("retry_attempt").(int)

		err := c.Next()
		status := c.Response().StatusCode()
		if !shouldRetryStatus(status) || attempt >= cfg.MaxRetries {
			return err
		}

		c.Locals("retry_attempt", attempt+1)
		backoff := baseBackoff * time.Duration(1<<attempt)
		timer := time.NewTimer(backoff)
		defer timer.Stop()
		select {
		case <-timer.C:
		case <-c.UserContext().Done():
			err := context.Cause(c.UserContext())
			if err == nil {
				err = c.UserContext().Err()
			}
			return err
		}
		c.Response().Reset()
		return c.RestartRouting()
	}
}

func shouldRetryStatus(status int) bool {
	switch status {
	case fiber.StatusTooManyRequests,
		fiber.StatusBadGateway,
		fiber.StatusServiceUnavailable,
		fiber.StatusGatewayTimeout:
		return true
	default:
		return false
	}
}
