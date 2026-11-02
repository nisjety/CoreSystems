package http

import "github.com/gin-gonic/gin"

// WS12-S5: the message-lifecycle Control routes, registered through the P-UC1
// seam. Scopes are in scopes_space_message_lifecycle.go; handlers in
// space_message_lifecycle.go.
//
//	POST /api/v1/internal/spaces/thread-revise-decision    issuer, gateway-delegated (spaces:issue)
//	GET  /api/v1/internal/spaces/:space_ref/message-policy UI hints, gateway-delegated (spaces:resolve)
//	PUT  /api/v1/internal/spaces/message-policy            operator path (spaces:policy:write)
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/thread-revise-decision", s.requireVerifiedSpaceResolver, s.issueThreadReviseDecision)
		spacesGroup.GET("/:space_ref/message-policy", s.requireVerifiedSpaceResolver, s.getSpaceMessagePolicy)
		spacesGroup.PUT("/message-policy", s.requireSpacePolicyWriter, s.upsertSpaceMessagePolicy)
	})
}
