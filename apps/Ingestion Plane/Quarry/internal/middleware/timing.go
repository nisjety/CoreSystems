package middleware

import (
	"math/rand"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
)

type TimingConfig struct {
	Enabled  bool
	MinDelay time.Duration
	MaxDelay time.Duration
}

func Timing(cfg TimingConfig) fiber.Handler {
	if !cfg.Enabled {
		return func(c *fiber.Ctx) error { return c.Next() }
	}

	minDelay := cfg.MinDelay
	maxDelay := cfg.MaxDelay
	if minDelay < 0 {
		minDelay = 0
	}
	if maxDelay < minDelay {
		maxDelay = minDelay
	}

	rng := rand.New(rand.NewSource(time.Now().UnixNano()))
	var rngMu sync.Mutex

	return func(c *fiber.Ctx) error {
		path := c.Path()
		if strings.HasPrefix(path, "/health") || strings.HasPrefix(path, "/ready") || strings.HasPrefix(path, "/metrics") {
			return c.Next()
		}

		delay := minDelay
		if maxDelay > minDelay {
			rngMu.Lock()
			delay += time.Duration(rng.Int63n(int64(maxDelay - minDelay + 1)))
			rngMu.Unlock()
		}

		if delay > 0 {
			time.Sleep(delay)
		}

		return c.Next()
	}
}
