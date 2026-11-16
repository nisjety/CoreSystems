package http

import "github.com/gin-gonic/gin"

// WS17-S4: the personal agent registry and the room agent routes, registered through
// the P-UC1 seam. Each route names its own delegation guard. Scopes are in
// scopes_space_agents.go and service_auth.go.
func init() {
	registerFeatureRoute(func(s *Server, _ *gin.RouterGroup, internal *gin.RouterGroup) {
		internal.POST("/agents/subjects", requireGatewayDelegation(agentsManageScope), s.registerAgentSubject)
		internal.GET("/agents/subjects", requireGatewayDelegation(agentsManageScope), s.listAgentSubjects)
		internal.GET("/agents/subjects/:agent_subject_id", requireGatewayDelegation(agentsManageScope), s.getAgentSubject)
		internal.PUT("/agents/subjects/:agent_subject_id/access", requireGatewayDelegation(agentsManageScope), s.updateAgentAccess)
	})
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/:space_ref/agent-candidates", requireGatewayDelegation(spaceAgentsManageScope), s.spaceAgentCandidates)
		spacesGroup.POST("/:space_ref/agent-admissions", requireGatewayDelegation(spaceAgentsManageScope), s.admitSpaceAgent)
		spacesGroup.DELETE("/:space_ref/agent-admissions/:agent_subject_id", requireGatewayDelegation(spaceAgentsManageScope), s.withdrawSpaceAgent)
	})
}
