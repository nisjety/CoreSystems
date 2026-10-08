package http

import (
	"context"
	"errors"
	"log"
	"net/http"
	"strings"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// retentionPostureReader is the seam the posture route depends on.
// *org.Service satisfies it. Keeping it to one method lets the route be tested
// through the real router and the real service-auth middleware without a
// database.
type retentionPostureReader interface {
	GetRetentionPosture(ctx context.Context, orgID string) (*orgcore.RetentionPosture, error)
}

// retentionPostureTimeout bounds the lookup so a slow database cannot hold a
// token mint open past Auth Core's own request timeout.
const retentionPostureTimeout = 3 * time.Second

// getRetentionPostureInternal serves
// GET /internal/orgs/:orgId/retention-posture.
//
// It answers one machine caller, Auth Core's retention resolver, with the two
// booleans it needs to stamp a token's zdr claim: the org admin's stored ZDR
// intent and whether the org's current plan is entitled. Access requires the
// org:retention:read:any scope (serviceScopesForRequest), which is separate
// from org:read:any so the holder cannot read anything else about the org.
//
// The response is computed server-side from the database (never from a cache)
// and marked no-store: a posture read decides whether content is retained.
// A missing or soft-deleted org is a definitive 404; any other failure is a 500
// with no detail, which the resolver treats as "posture unknown" and never as
// "not ZDR".
func (s *Server) getRetentionPostureInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	if s.retentionPosture == nil {
		log.Printf("retention posture: no reader configured for org=%s", orgID)
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "retention posture unavailable"})
		return
	}

	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionPostureTimeout)
	defer cancel()
	posture, err := s.retentionPosture.GetRetentionPosture(ctx, orgID)
	if err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
			return
		}
		log.Printf("retention posture: lookup failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read retention posture"})
		return
	}
	c.JSON(http.StatusOK, posture)
}
