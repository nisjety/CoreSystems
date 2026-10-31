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

// agentPolicyReader is the seam the agent-policy route depends on.
// *org.Service satisfies it; one method lets the route be tested through the
// real router and service-auth middleware without a database.
type agentPolicyReader interface {
	GetAgentLearningPolicy(ctx context.Context, orgID string) (*orgcore.AgentLearningPolicy, error)
}

// agentPolicyTimeout bounds the lookup so a slow database cannot hold
// session-core's poll open.
const agentPolicyTimeout = 3 * time.Second

// getAgentPolicyInternal serves GET /internal/orgs/:orgId/agent-policy.
//
// It answers one machine caller, session-core's policy poller, with the org's
// memory and learning-review policy: memory allow/disabled, learning review
// disabled/proposals, and the revision of the last write. Access requires the
// org:agent-policy:read:any scope (serviceScopesForRequest), separate from
// org:read:any so the holder reads nothing else about the org.
//
// The answer is computed from the database, never from a cache, and marked
// no-store: it decides whether members' conversations are mined. A missing or
// soft-deleted org is a definitive 404; any other failure is a 500 with no
// detail, which the poller treats as "policy unknown" and never as "allowed".
func (s *Server) getAgentPolicyInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	if s.agentPolicy == nil {
		log.Printf("agent policy: no reader configured for org=%s", orgID)
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "agent policy unavailable"})
		return
	}

	ctx, cancel := context.WithTimeout(c.Request.Context(), agentPolicyTimeout)
	defer cancel()
	policy, err := s.agentPolicy.GetAgentLearningPolicy(ctx, orgID)
	if err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
			return
		}
		log.Printf("agent policy: lookup failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read agent policy"})
		return
	}
	c.JSON(http.StatusOK, policy)
}
