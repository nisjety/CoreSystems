package http

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
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

// grantView is the JSON shape returned for a single grant. It mirrors the
// resource_grants row but never exposes anything beyond the share relationship
// the ShareDialog needs (who it's shared with, at what role, by whom, when).
func grantView(g *users.ResourceGrant) gin.H {
	return gin.H{
		"grant_id":      g.GrantID,
		"org_id":        g.OrgID,
		"resource_type": g.ResourceType,
		"resource_id":   g.ResourceID,
		"subject_type":  g.SubjectType,
		"subject_id":    g.SubjectID,
		"role":          g.Role,
		"granted_by":    g.GrantedBy,
		"granted_at":    g.GrantedAt,
	}
}

// authzGrant: POST /api/v1/internal/authz/grant
// Body: { org_id, resource_type, resource_id, subject_id, subject_type?, role?, granted_by }
// Upserts an explicit grant (MVP: subject_type=user, role=view). The Grant repo
// method is the single authority retrieval + documents-api enforce against, so
// the ShareDialog writes here rather than to any display-only flag. Idempotent.
func (s *Server) authzGrant(c *gin.Context) {
	var req struct {
		OrgID        string `json:"org_id"`
		ResourceType string `json:"resource_type"`
		ResourceID   string `json:"resource_id"`
		SubjectID    string `json:"subject_id"`
		SubjectType  string `json:"subject_type"`
		Role         string `json:"role"`
		GrantedBy    string `json:"granted_by"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ResourceType = strings.TrimSpace(req.ResourceType)
	req.ResourceID = strings.TrimSpace(req.ResourceID)
	req.SubjectID = strings.TrimSpace(req.SubjectID)
	if req.OrgID == "" || req.ResourceType == "" || req.ResourceID == "" || req.SubjectID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org_id, resource_type, resource_id and subject_id are required"})
		return
	}
	if strings.TrimSpace(req.SubjectType) == "" {
		req.SubjectType = "user"
	}
	if strings.TrimSpace(req.Role) == "" {
		req.Role = "view"
	}
	if s.aclRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "authz facade not configured"})
		return
	}

	out, err := s.aclRepo.Grant(c.Request.Context(), &users.ResourceGrant{
		OrgID:        req.OrgID,
		ResourceType: req.ResourceType,
		ResourceID:   req.ResourceID,
		SubjectType:  req.SubjectType,
		SubjectID:    req.SubjectID,
		Role:         req.Role,
		GrantedBy:    strings.TrimSpace(req.GrantedBy),
	})
	if err != nil {
		// Grant fails closed on non-grantable types / unsupported subjects /
		// bad role — surface as 400 so the UI shows a clear message.
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	// Evict the Data Plane retrieval visibility cache for (subject_id, org_id) so
	// the recipient sees the newly-shared resource within one query (TTL backstop).
	if s.publisher != nil {
		s.publisher.PublishResourceGrantsChanged(c.Request.Context(),
			out.GrantID, out.OrgID, out.ResourceType, out.ResourceID,
			out.SubjectType, out.SubjectID, out.Role, "grant")
	}
	c.JSON(http.StatusOK, grantView(out))
}

// authzRevoke: DELETE /api/v1/internal/authz/grant?org_id=&resource_type=&resource_id=&subject_id=[&subject_type=user]
// Removes an explicit grant (the ShareDialog "remove" action). Idempotent.
func (s *Server) authzRevoke(c *gin.Context) {
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

	if err := s.aclRepo.Revoke(c.Request.Context(), orgID, resourceType, resourceID, subjectType, subjectID); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to revoke grant"})
		return
	}
	// Evict the Data Plane visibility cache so the revoke takes effect within one
	// query (the DP keys eviction on subject_id+org_id; grant_id/role unused here).
	if s.publisher != nil {
		s.publisher.PublishResourceGrantsChanged(c.Request.Context(),
			"", orgID, resourceType, resourceID, subjectType, subjectID, "", "revoke")
	}
	c.JSON(http.StatusOK, gin.H{"revoked": true})
}

// authzGrantsByResource: GET /api/v1/internal/authz/grants?org_id=&resource_type=&resource_id=
// Lists every explicit grant on a resource — the ShareDialog's live "shared with"
// list. Read from the same resource_grants authority retrieval enforces.
func (s *Server) authzGrantsByResource(c *gin.Context) {
	orgID := strings.TrimSpace(c.Query("org_id"))
	resourceType := strings.TrimSpace(c.Query("resource_type"))
	resourceID := strings.TrimSpace(c.Query("resource_id"))
	if orgID == "" || resourceType == "" || resourceID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org_id, resource_type and resource_id are required"})
		return
	}
	if s.aclRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "authz facade not configured"})
		return
	}

	grants, err := s.aclRepo.ListByResource(c.Request.Context(), orgID, resourceType, resourceID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list grants"})
		return
	}
	out := make([]gin.H, 0, len(grants))
	for _, g := range grants {
		out = append(out, grantView(g))
	}
	c.JSON(http.StatusOK, gin.H{"grants": out})
}
