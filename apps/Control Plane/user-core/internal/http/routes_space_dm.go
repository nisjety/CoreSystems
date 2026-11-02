package http

import "github.com/gin-gonic/gin"

// WS12-S1: the direct-message eligibility pre-check, registered through the
// P-UC1 seam. It is a gateway-delegated resolver route (scope spaces:resolve,
// scopes_space_dm.go) and writes nothing.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/dm-eligibility", s.requireVerifiedSpaceResolver, s.dmEligibility)
	})
}
