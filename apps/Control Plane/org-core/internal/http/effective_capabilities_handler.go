package http

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

// getEffectiveCapabilities is a machine-to-machine internal read:
// GET /internal/orgs/:orgId/roles/:roleName/capabilities. roleName is
// exactly the value already stored in organization_members.role for the
// caller's own verified membership (resolved by the gateway before this is
// ever reached, never trusted from an end user) -- see
// rbac.Repository.EffectiveCapabilities for why this needs only the role
// string, not a fresh membership lookup.
func (s *Server) getEffectiveCapabilities(c *gin.Context) {
	orgID := c.Param("orgId")
	roleName := c.Param("roleName")
	if orgID == "" || roleName == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id and role are required"})
		return
	}
	capabilities, err := s.rbacRepo.EffectiveCapabilities(c.Request.Context(), orgID, roleName)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve effective capabilities"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"capabilities": capabilities})
}
