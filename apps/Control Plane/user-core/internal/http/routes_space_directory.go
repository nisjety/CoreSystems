package http

import "github.com/gin-gonic/gin"

// WS04-S8: room join, settings, directory rooms and the directory policy,
// registered through the P-UC1 seam. join and settings are ACT routes
// (requireSpaceMembershipActor); directory rooms are a DIR read; the policy
// route is the operator path (requireSpacePolicyWriter). Scopes are in
// scopes_space_directory.go.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/:space_ref/join", s.requireSpaceMembershipActor, s.joinSpaceRoom)
		spacesGroup.PATCH("/:space_ref/settings", s.requireSpaceMembershipActor, s.updateSpaceRoomSettings)
		spacesGroup.PUT("/directory-policy", s.requireSpacePolicyWriter, s.upsertSpaceDirectoryPolicy)
	})
	registerFeatureRoute(func(s *Server, _ *gin.RouterGroup, internal *gin.RouterGroup) {
		internal.GET("/directory/rooms", s.requireDirectoryReader, s.listDirectoryRooms)
	})
}
