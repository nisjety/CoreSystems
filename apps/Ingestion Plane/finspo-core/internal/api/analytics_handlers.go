package api

import (
	"context"
	"strconv"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/finspo/internal/auth"
	"github.com/triodelab/finspo/internal/store"
)

// AnalyticsReader is the narrow surface the analytics handlers need.
// Implemented by *store.Analytics.
type AnalyticsReader interface {
	Largest(ctx context.Context, organizationID string, limit int, minSizeBytes int64) ([]store.LargestItem, error)
	Inactive(ctx context.Context, organizationID string, olderThan time.Duration, limit int) ([]store.InactiveItem, error)
	BySite(ctx context.Context, organizationID string) ([]store.SiteAggregate, error)
	Duplicates(ctx context.Context, organizationID string, minCount int, minSizeBytes int64, maxGroups int) ([]store.DuplicateGroup, error)
}

const (
	defaultAnalyticsLimit = 50
	maxAnalyticsLimit     = 500
	defaultInactiveAge    = 180 * 24 * time.Hour
	maxDuplicateGroups    = 200
)

func registerAnalyticsRoutes(g fiber.Router, reader AnalyticsReader) {
	if reader == nil {
		return
	}
	g.Get("/analytics/largest", largestHandler(reader))
	g.Get("/analytics/inactive", inactiveHandler(reader))
	g.Get("/analytics/by-site", bySiteHandler(reader))
	g.Get("/analytics/duplicates", duplicatesHandler(reader))
}

func largestHandler(reader AnalyticsReader) fiber.Handler {
	return func(c *fiber.Ctx) error {
		limit := boundedIntQuery(c, "limit", defaultAnalyticsLimit, 1, maxAnalyticsLimit)
		minSize := int64Query(c, "min_size", 0)
		items, err := reader.Largest(c.UserContext(), auth.OrganizationID(c), limit, minSize)
		if err != nil {
			return serverError(c, "analytics: largest", err)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"limit":    limit,
				"min_size": minSize,
				"count":    len(items),
				"items":    items,
			},
		})
	}
}

func inactiveHandler(reader AnalyticsReader) fiber.Handler {
	return func(c *fiber.Ctx) error {
		limit := boundedIntQuery(c, "limit", defaultAnalyticsLimit, 1, maxAnalyticsLimit)
		ageStr := c.Query("older_than", "")
		olderThan := defaultInactiveAge
		if ageStr != "" {
			if d, err := time.ParseDuration(ageStr); err == nil && d > 0 {
				olderThan = d
			} else {
				return clientError(c, fiber.StatusBadRequest, "invalid older_than (use Go duration, e.g. 180d-equivalent like 4320h)")
			}
		}
		items, err := reader.Inactive(c.UserContext(), auth.OrganizationID(c), olderThan, limit)
		if err != nil {
			return serverError(c, "analytics: inactive", err)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"limit":      limit,
				"older_than": olderThan.String(),
				"count":      len(items),
				"items":      items,
			},
		})
	}
}

func bySiteHandler(reader AnalyticsReader) fiber.Handler {
	return func(c *fiber.Ctx) error {
		aggregates, err := reader.BySite(c.UserContext(), auth.OrganizationID(c))
		if err != nil {
			return serverError(c, "analytics: by-site", err)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"count":      len(aggregates),
				"aggregates": aggregates,
			},
		})
	}
}

func duplicatesHandler(reader AnalyticsReader) fiber.Handler {
	return func(c *fiber.Ctx) error {
		minCount := boundedIntQuery(c, "min_count", 2, 2, 1000)
		minSize := int64Query(c, "min_size", 0)
		maxGroups := boundedIntQuery(c, "max_groups", 100, 1, maxDuplicateGroups)
		groups, err := reader.Duplicates(c.UserContext(), auth.OrganizationID(c), minCount, minSize, maxGroups)
		if err != nil {
			return serverError(c, "analytics: duplicates", err)
		}
		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"min_count":  minCount,
				"min_size":   minSize,
				"max_groups": maxGroups,
				"count":      len(groups),
				"groups":     groups,
			},
		})
	}
}

func boundedIntQuery(c *fiber.Ctx, key string, def, lo, hi int) int {
	raw := c.Query(key, "")
	if raw == "" {
		return def
	}
	v, err := strconv.Atoi(raw)
	if err != nil {
		return def
	}
	if v < lo {
		return lo
	}
	if v > hi {
		return hi
	}
	return v
}

func int64Query(c *fiber.Ctx, key string, def int64) int64 {
	raw := c.Query(key, "")
	if raw == "" {
		return def
	}
	v, err := strconv.ParseInt(raw, 10, 64)
	if err != nil || v < 0 {
		return def
	}
	return v
}
