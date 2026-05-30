package middleware

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
)

// RequestContextTimeout attaches a deadline to Fiber's user context instead of
// wrapping the handler in the deprecated timeout middleware. Handlers that pass
// c.UserContext() into blocking work then get normal context cancellation without
// the race-prone goroutine wrapper used by fiber/middleware/timeout.New.
func RequestContextTimeout(timeout time.Duration) fiber.Handler {
	if timeout <= 0 {
		return func(c *fiber.Ctx) error { return c.Next() }
	}

	return func(c *fiber.Ctx) error {
		// SSE streams are intentionally long-lived; forcing the default request
		// timeout onto them would terminate active job streams mid-crawl.
		if strings.HasSuffix(c.Path(), "/stream") {
			return c.Next()
		}

		ctx, cancel := context.WithTimeout(c.UserContext(), timeout)
		defer cancel()
		c.SetUserContext(ctx)

		err := c.Next()
		if errors.Is(err, context.DeadlineExceeded) {
			return fiber.ErrRequestTimeout
		}
		return err
	}
}
