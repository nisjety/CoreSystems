package api

import (
	"context"
	"errors"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/finspo/internal/auth"
	"github.com/triodelab/finspo/internal/store"
)

// Recommender is the narrow surface the recommendations handler needs.
// Implemented by *store.Recommendations.
type Recommender interface {
	Duplicates(ctx context.Context, organizationID string, minSizeBytes int64, maxGroups int) ([]store.RecommendationDraft, error)
	Inactive(ctx context.Context, organizationID string, olderThan time.Duration, limit int) (store.RecommendationDraft, error)
}

func registerRecommendationRoutes(g fiber.Router, rec Recommender) {
	if rec == nil {
		return
	}
	g.Get("/recommendations", recommendationsHandler(rec))
}

// recommendationsHandler returns pre-filled proposal DRAFTS. It never persists
// anything — the operator POSTs a draft to /proposals to actually create one.
func recommendationsHandler(rec Recommender) fiber.Handler {
	return func(c *fiber.Ctx) error {
		orgID := auth.OrganizationID(c)
		minSize := int64Query(c, "min_size", 0)
		maxGroups := boundedIntQuery(c, "max_groups", 50, 1, maxDuplicateGroups)
		limit := boundedIntQuery(c, "inactive_limit", defaultAnalyticsLimit, 1, maxAnalyticsLimit)

		olderThan := defaultInactiveAge
		if raw := c.Query("older_than", ""); raw != "" {
			d, err := time.ParseDuration(raw)
			if err != nil || d <= 0 {
				return clientError(c, fiber.StatusBadRequest, "invalid older_than (Go duration)")
			}
			olderThan = d
		}

		dupDrafts, err := rec.Duplicates(c.UserContext(), orgID, minSize, maxGroups)
		if err != nil {
			return serverError(c, "recommend duplicates", err)
		}

		drafts := append([]store.RecommendationDraft{}, dupDrafts...)

		inactiveDraft, err := rec.Inactive(c.UserContext(), orgID, olderThan, limit)
		switch {
		case err == nil:
			drafts = append(drafts, inactiveDraft)
		case errors.Is(err, store.ErrNotFound):
			// no inactive files — fine, just omit the archive draft
		default:
			return serverError(c, "recommend inactive", err)
		}

		var reclaim int64
		for _, d := range drafts {
			reclaim += d.EstimatedBytes
		}

		return c.JSON(fiber.Map{
			"success": true,
			"data": fiber.Map{
				"count":                 len(drafts),
				"estimated_bytes_total": reclaim,
				"drafts":                drafts,
			},
		})
	}
}
