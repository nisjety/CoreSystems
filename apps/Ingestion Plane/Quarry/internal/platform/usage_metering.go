package platform

import (
	"context"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"

	quarrynats "github.com/triodelab/quarry/internal/nats"
)

// ─── Usage metering (post-request) ──────────────────────────────────────────

// UsageMeteringConfig configures the usage metering middleware.
type UsageMeteringConfig struct {
	Publisher     *quarrynats.SharedPublisher
	BillingClient *BillingClient
}

// UsageMeteringMiddleware is a Fiber middleware that, AFTER the handler
// completes, records usage against billing-core and publishes a NATS event.
//
// It must run after AuthBridgeMiddleware (needs Principal).
// Install with app.Use() so it wraps all /v1 handlers.
func UsageMeteringMiddleware(cfg UsageMeteringConfig) fiber.Handler {
	return func(c *fiber.Ctx) error {
		start := time.Now()

		// Run the rest of the chain first.
		err := c.Next()

		// Only meter successful requests (2xx).
		status := c.Response().StatusCode()
		if status < 200 || status >= 300 {
			return err
		}

		principal := GetPrincipal(c)
		if principal == nil || principal.OrganizationID == "internal" {
			return err
		}

		duration := time.Since(start)
		orgID := principal.OrganizationID
		tier := principal.Tier
		path := c.Path()
		method := c.Method()

		// Determine credit cost based on endpoint.
		credits := classifyCreditCost(method, path)
		if credits <= 0 {
			return err
		}

		// Async: record usage with billing-core (fire-and-forget).
		go func() {
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()

			metadata := map[string]interface{}{
				"method":     method,
				"path":       path,
				"tier":       tier,
				"durationMs": duration.Milliseconds(),
			}

			if recordErr := cfg.BillingClient.RecordUsage(ctx, orgID, "crawl_credits", int64(credits), metadata); recordErr != nil {
				log.Warn().Err(recordErr).
					Str("org_id", orgID).
					Int("credits", credits).
					Msg("Failed to record usage with billing-core")
			}
		}()

		// Async: publish NATS usage event.
		go func() {
			if cfg.Publisher == nil {
				return
			}

			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()

			cfg.Publisher.PublishUsageRecorded(ctx, orgID, "crawl_credits", int64(credits), map[string]interface{}{
				"method":     method,
				"path":       path,
				"tier":       tier,
				"durationMs": duration.Milliseconds(),
			})
		}()

		return err
	}
}

// classifyCreditCost assigns a credit cost to each API operation.
// Heavier operations (multi-page crawl, batch) cost more credits.
func classifyCreditCost(method, path string) int {
	switch {
	// Read-only or lightweight
	case method == "GET" && (path == "/v1/modules" || path == "/health" || path == "/readiness"):
		return 0
	// Search queries
	case method == "POST" && contains(path, "/search"):
		return 1
	// Single page scrape
	case method == "POST" && contains(path, "/scrape"):
		return 1
	// Multi-page crawl
	case method == "POST" && contains(path, "/crawl"):
		return 5
	// Map site
	case method == "POST" && contains(path, "/map"):
		return 2
	// Structured extraction
	case method == "POST" && contains(path, "/extract"):
		return 3
	// Batch operations
	case method == "POST" && contains(path, "/batch"):
		return 10
	// Interactive sessions
	case method == "POST" && contains(path, "/interact"):
		return 2
	// Agent mode
	case method == "POST" && contains(path, "/agent"):
		return 5
	default:
		return 0
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > len(substr) && findSubstring(s, substr))
}

func findSubstring(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
