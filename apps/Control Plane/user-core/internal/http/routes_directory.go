package http

import "github.com/gin-gonic/gin"

// WS04-S3: people-directory route, registered through the P-UC1 seam. The
// internal group already sits behind requireServicePrincipal; the route asserts
// its own scope and delegation in requireDirectoryReader.
func init() {
	registerFeatureRoute(func(s *Server, _ *gin.RouterGroup, internal *gin.RouterGroup) {
		internal.GET("/directory/people", s.requireDirectoryReader, s.searchDirectoryPeople)
	})
}
