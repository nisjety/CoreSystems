package http

import "github.com/gin-gonic/gin"

// W3-DEC-ORGAGENT (D19): the org-admin routes of the org-visible agent definitions, registered
// through the P-UC1 seam. The scope is agents:manage (service_auth.go maps
// /api/v1/internal/agents/**). requireOrgAgentAdmin adds the canonical owner or admin role.
func init() {
	registerFeatureRoute(func(s *Server, _ *gin.RouterGroup, internal *gin.RouterGroup) {
		orgAdmin := s.requireOrgAgentAdmin()
		internal.GET("/agents/org-subjects", orgAdmin, s.listOrgAgentSubjects)
		internal.PUT("/agents/org-subjects/:agent_subject_id/suspend", orgAdmin, s.suspendOrgAgentSubject)
		internal.PUT("/agents/org-subjects/:agent_subject_id/reactivate", orgAdmin, s.reactivateOrgAgentSubject)
		internal.DELETE("/agents/org-subjects/:agent_subject_id", orgAdmin, s.retireOrgAgentSubject)
	})
}
