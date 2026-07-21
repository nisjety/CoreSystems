package http

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
)

// promoteMemberSuccession is the internal, service-principal-gated surface a
// departing sole owner/admin's self-erasure pre-flight calls before the
// erasure saga starts (user-core's Service.EnsureSuccession in
// internal/users/gdpr_succession.go). It ONLY promotes an EXISTING active
// member into a base role (owner|admin) — it never invites, removes, or
// demotes anyone else.
//
// Machine-to-machine (no acting end-user), so the membership guard does not
// apply; access is scoped to a registered service principal (e.g. "user-core")
// holding the org:membership:succession:any scope in ORG_CORE_SERVICE_CREDENTIALS.
//
// POST /internal/orgs/:orgId/members/:userId/succession   Body: {"role":"owner"|"admin"}
func (s *Server) promoteMemberSuccession(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("orgId"))
	userID := strings.TrimSpace(c.Param("userId"))

	var req struct {
		Role string `json:"role" binding:"required"`
	}
	if orgID == "" || userID == "" || c.ShouldBindJSON(&req) != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "orgId, userId, and role are required"})
		return
	}

	role := strings.ToLower(strings.TrimSpace(req.Role))
	if role != "owner" && role != "admin" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "role must be owner or admin"})
		return
	}

	if err := s.orgService.PromoteMemberSuccession(c.Request.Context(), orgID, userID, role); err != nil {
		c.JSON(http.StatusConflict, gin.H{"error": "failed to promote successor"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"ok": true})
}
