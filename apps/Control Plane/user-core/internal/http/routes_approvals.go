package http

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

// WS10-S2: Control-held approval authority, registered through the P-UC1 seam.
// Scopes are in scopes_approvals.go; handlers are in approval_grants.go (the
// gateway-delegated manager surface) and approval_gate.go (the execution-core
// surface). Every route answers in the typed envelope
// {"error":{"code","message","details"}}.
//
// Gateway-delegated (verified user): rule create/list/revoke, Space ceiling.
// Service (execution-core): gate view, rule use, continuation check.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/approval-grants", s.requireVerifiedSpaceResolver, s.createApprovalGrant)
		spacesGroup.GET("/:space_ref/approval-grants", s.requireVerifiedSpaceResolver, s.listApprovalGrants)
		spacesGroup.DELETE("/:space_ref/approval-grants/:rule_id", s.requireVerifiedSpaceResolver, s.revokeApprovalGrant)
		spacesGroup.GET("/:space_ref/approval-policy", s.requireVerifiedSpaceResolver, s.getApprovalPolicy)
		spacesGroup.PUT("/:space_ref/approval-policy", s.requireVerifiedSpaceResolver, s.putApprovalPolicy)

		spacesGroup.POST("/tool-gate-view", s.requireApprovalExecutionScope(spaceToolGateViewScope), s.issueToolGateView)
		spacesGroup.POST("/approval-grants/use", s.requireApprovalExecutionScope(spaceApprovalGrantUseScope), s.useApprovalGrant)
		spacesGroup.POST("/approval-continuation-check", s.requireApprovalExecutionScope(spaceApprovalContinuationScope), s.checkApprovalContinuation)
	})
}

// requireApprovalExecutionScope admits only Execution Core's service identity
// holding the named scope. Each of the three routes has its own scope, so the
// credential that may read a gate view cannot spend a rule, and neither can
// check a continuation.
func (s *Server) requireApprovalExecutionScope(scope string) gin.HandlerFunc {
	return func(c *gin.Context) {
		if c.GetString("auth_method") != "service_principal" ||
			c.GetString("service_id") != executionCorePrincipal ||
			!hasServiceScope(c, scope) {
			abortTypedError(c, http.StatusForbidden, "service_scope_required",
				"Execution Core approval principal required", map[string]any{"scope": scope})
			return
		}
		c.Next()
	}
}
