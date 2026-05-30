package platform

import (
	"context"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog/log"

	quarrynats "github.com/triodelab/quarry/internal/nats"
)

// QuotaGateConfig configures the pre-request quota gate.
type QuotaGateConfig struct {
	BillingClient *BillingClient
	Publisher     *quarrynats.SharedPublisher
	Metric        string // e.g. "crawl_credits"
}

// QuotaGateMiddleware is a Fiber middleware that checks billing-core before
// executing expensive operations. If the org's quota is exhausted, the
// request is rejected with 402 Payment Required.
//
// Install AFTER AuthBridgeMiddleware. Only applies to POST endpoints
// (scrape, crawl, extract, batch, etc.). GET requests pass through.
func QuotaGateMiddleware(cfg QuotaGateConfig) fiber.Handler {
	metric := cfg.Metric
	if metric == "" {
		metric = "crawl_credits"
	}

	return func(c *fiber.Ctx) error {
		// Only gate mutating / expensive operations.
		if c.Method() != "POST" {
			return c.Next()
		}

		principal := GetPrincipal(c)
		if principal == nil {
			return c.Next()
		}

		// Internal/service calls bypass quota.
		if principal.Tier == "internal" {
			return c.Next()
		}

		orgID := principal.OrganizationID

		// Estimate credits needed for this request.
		estimatedCredits := int64(classifyCreditCost(c.Method(), c.Path()))
		if estimatedCredits <= 0 {
			return c.Next()
		}

		ctx, cancel := context.WithTimeout(c.UserContext(), 3*time.Second)
		defer cancel()

		result, err := cfg.BillingClient.CheckQuota(ctx, orgID, metric, estimatedCredits)
		if err != nil {
			// Log but don't block — fail-open.
			log.Warn().Err(err).
				Str("org_id", orgID).
				Str("metric", metric).
				Msg("Quota check failed (fail-open)")
			return c.Next()
		}

		if !result.Allowed {
			log.Warn().
				Str("event", "quota_exceeded").
				Str("org_id", orgID).
				Str("tier", principal.Tier).
				Int64("limit", result.Limit).
				Int64("remaining", result.Remaining).
				Str("path", c.Path()).
				Msg("Quota exceeded — blocking request")

			// Publish quota exceeded event so other services can react.
			go func() {
				pubCtx, pubCancel := context.WithTimeout(context.Background(), 2*time.Second)
				defer pubCancel()
				if pubErr := cfg.Publisher.PublishQuotaExceeded(pubCtx, orgID, metric, result.Limit, result.Remaining); pubErr != nil {
					log.Warn().Err(pubErr).Msg("Failed to publish quota exceeded event")
				}
			}()

			return c.Status(fiber.StatusPaymentRequired).JSON(fiber.Map{
				"success":   false,
				"error":     "quota exceeded",
				"metric":    metric,
				"limit":     result.Limit,
				"remaining": result.Remaining,
				"resetAt":   result.ResetAt,
				"requestId": c.GetRespHeader("X-Request-ID"),
			})
		}

		return c.Next()
	}
}
