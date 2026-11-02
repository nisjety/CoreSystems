package http

import "github.com/gin-gonic/gin"

// WS08-S1: POST /api/v1/internal/spaces/run-wake-decision, registered through
// the P-UC1 seam. It is a gateway-delegated issuer route like
// thread-append-decision (scope spaces:issue, scopes_run_wake.go) and is unused
// until the BFF wake branch (WS08-S4) calls it.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/run-wake-decision", s.requireVerifiedSpaceResolver, s.issueRunWakeDecision)
	})
}
