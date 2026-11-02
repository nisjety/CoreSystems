package http

import "github.com/gin-gonic/gin"

// WS10-S5a: the Space tool policy routes, registered through the P-UC1 seam.
// Scopes are in scopes_tool_policy.go. All three are gateway-delegated (verified
// user): the person is proven by the delegation, never by the body.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.GET("/:space_ref/tool-policy", s.requireVerifiedSpaceResolver, s.getToolPolicy)
		spacesGroup.PUT("/:space_ref/tool-policy", s.requireVerifiedSpaceResolver, s.putSpaceToolPolicy)
		spacesGroup.PUT("/:space_ref/agents/:agent_subject_id/tool-policy", s.requireVerifiedSpaceResolver, s.putAgentToolPolicy)
	})
}
