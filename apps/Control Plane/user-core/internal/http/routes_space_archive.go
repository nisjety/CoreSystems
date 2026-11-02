package http

import "github.com/gin-gonic/gin"

// WS04-S7: room archive and restore routes, registered through the P-UC1 seam.
// Both are ACT routes (requireSpaceMembershipActor), so the writer credential
// is also their kill switch. Scopes are in scopes_space_archive.go.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/:space_ref/archive", s.requireSpaceMembershipActor, s.archiveSpaceRoom)
		spacesGroup.POST("/:space_ref/unarchive", s.requireSpaceMembershipActor, s.unarchiveSpaceRoom)
	})
}
