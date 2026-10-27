package http

import "github.com/gin-gonic/gin"

// WS03-S3: org-admin Space policy routes, registered through the P-UC1 seam.
// The group is /api/v1/internal/spaces (already behind requireServicePrincipal);
// each route asserts its own scope and the canonical org role.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spaces *gin.RouterGroup) {
		spaces.GET("/org-policy", s.requireSpaceOrgPolicyAdmin(false), s.getSpaceOrgPolicy)
		spaces.GET("/org-policy/events", s.requireSpaceOrgPolicyAdmin(false), s.listSpaceOrgPolicyEvents)
		spaces.PUT("/org-policy/entitlements", s.requireSpaceOrgPolicyAdmin(true), s.putSpaceOrgPolicyEntitlements)
	})
}
