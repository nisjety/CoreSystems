package http

import "github.com/gin-gonic/gin"

// WS04-S4: the room membership writer routes, registered through the P-UC1
// seam. Each route names its own guard; scopes are in
// scopes_space_membership_ops.go.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/:space_ref/members", s.requireSpaceMembershipActor, s.addSpaceMember)
		spacesGroup.PATCH("/:space_ref/members/:subject_id", s.requireSpaceMembershipActor, s.changeSpaceMemberRole)
		spacesGroup.DELETE("/:space_ref/members/:subject_id", s.requireSpaceMembershipActor, s.removeSpaceMember)
		spacesGroup.GET("/:space_ref/audience-candidates", s.requireSpaceAudienceReader, s.spaceAudienceCandidates)
	})
}
