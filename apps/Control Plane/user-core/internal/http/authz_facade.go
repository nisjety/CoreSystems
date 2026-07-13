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

// requireServicePrincipal protects internal operations whose explicit service
// scope is sufficient and which do not impersonate a delegated end user.
func (s *Server) requireServicePrincipal(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "service principal required"})
		return
	}
	c.Next()
}

// requireVerifiedAuthzDelegation permits only claim-pinned grant reads. The
// service credential authenticates the workload while the request signature
// binds its method, URI/resource selectors, body digest, tenant, and user.
// Grant listing/mutations remain disabled until the owning Data service proves
// the actor may administer the selected resource.
func (s *Server) requireVerifiedAuthzDelegation(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" || !c.GetBool("delegation_verified") || !c.GetBool("delegated_user_proof_verified") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "verified service delegation required"})
		return
	}
	if c.GetString("delegation_version") != "v2" || c.GetString("delegation_zdr") != "true" {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "bounded authz delegation required"})
		return
	}
	if c.Request.Method != http.MethodGet && c.Request.Method != http.MethodHead {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "verified resource-owner delegation required"})
		return
	}
	path := c.Request.URL.Path
	if path != "/api/v1/internal/authz/visible" && path != "/api/v1/internal/authz/check" {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "verified resource-owner delegation required"})
		return
	}
	wantOperation := "authz:visible"
	if path == "/api/v1/internal/authz/check" {
		wantOperation = "authz:check"
	}
	if c.GetString("delegation_operation") != wantOperation || strings.TrimSpace(c.Query("resource_type")) != c.GetString("delegation_resource_type") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated operation or resource mismatch"})
		return
	}
	if path == "/api/v1/internal/authz/check" && strings.TrimSpace(c.Query("resource_id")) != c.GetString("delegation_resource_id") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated resource mismatch"})
		return
	}
	if strings.TrimSpace(c.Query("org_id")) != c.GetString("org_id") || strings.TrimSpace(c.Query("subject_id")) != c.GetString("user_id") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated tenant or subject mismatch"})
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
