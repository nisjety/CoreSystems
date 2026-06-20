package http

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
)

// authz_facade.go exposes the per-user authorization facade over HTTP. These are
// internal endpoints that Data Plane services call to resolve a viewer's
// explicit resource grants from resource_grants — the single grant authority.
// They never enumerate private/org-visible resources (the owning service
// resolves those from its own visibility columns), so the returned id set stays
// small.

// requireInternalKeyOnly rejects callers that authenticated with a user Bearer
// token. The global authContextMiddleware accepts EITHER a Bearer JWT OR the
// internal key; the facade leaks which documents are shared with an arbitrary
// subject_id, so it must be reachable only by trusted internal services (the
// internal key), never by an end-user token. Fail closed.
func (s *Server) requireInternalKeyOnly(c *gin.Context) {
	if c.GetString("auth_method") != "internal_key" {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "internal key required"})
		return
	}
	c.Next()
}

// authzVisible: GET /api/v1/internal/authz/visible?org_id=&subject_id=&resource_type=document[&subject_type=user]
// Returns { "ids": [...], "all_org": bool } — the explicit-grant resource ids the
// subject can see for that resource type in that org.
func (s *Server) authzVisible(c *gin.Context) {
	orgID := strings.TrimSpace(c.Query("org_id"))
	subjectID := strings.TrimSpace(c.Query("subject_id"))
	resourceType := strings.TrimSpace(c.Query("resource_type"))
	subjectType := strings.TrimSpace(c.Query("subject_type"))
	if subjectType == "" {
		subjectType = "user"
	}
	if orgID == "" || subjectID == "" || resourceType == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org_id, subject_id and resource_type are required"})
		return
	}
	if s.aclRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "authz facade not configured"})
		return
	}

	vis, err := s.aclRepo.ListVisible(c.Request.Context(), orgID, resourceType, subjectType, subjectID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list visible resources"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ids": vis.IDs, "all_org": vis.AllOrg})
}

// authzCheck: GET /api/v1/internal/authz/check?org_id=&resource_type=&resource_id=&subject_id=[&subject_type=user]
// Returns { "allowed": bool, "role": "view"|"edit"|"" }.
func (s *Server) authzCheck(c *gin.Context) {
	orgID := strings.TrimSpace(c.Query("org_id"))
	resourceType := strings.TrimSpace(c.Query("resource_type"))
	resourceID := strings.TrimSpace(c.Query("resource_id"))
	subjectID := strings.TrimSpace(c.Query("subject_id"))
	subjectType := strings.TrimSpace(c.Query("subject_type"))
	if subjectType == "" {
		subjectType = "user"
	}
	if orgID == "" || resourceType == "" || resourceID == "" || subjectID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org_id, resource_type, resource_id and subject_id are required"})
		return
	}
	if s.aclRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "authz facade not configured"})
		return
	}

	allowed, role, err := s.aclRepo.Check(c.Request.Context(), orgID, resourceType, resourceID, subjectType, subjectID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to check grant"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"allowed": allowed, "role": role})
}
