package api

import (
	"net/http"
	"strings"
)

const (
	// scopeComplianceRead lets the gateway call the compliance read API on behalf
	// of a user whose signed delegation carries an owner or admin role.
	scopeComplianceRead        = "audit:compliance:read:self"
	complianceGatewayPrincipal = "verevon-gateway"
)

// requireComplianceRole returns the request's service authorization when its
// signed delegation names an owner or admin, and otherwise writes the 403
// itself. The role comes from the v3 delegation the gateway signs, not from
// anything the browser sent; audit-core checks it again even though the gateway
// gates the same routes, so a gateway bug cannot open the audit log to a member.
func requireComplianceRole(w http.ResponseWriter, r *http.Request) (serviceAuthorization, bool) {
	authorization, ok := serviceAuthorizationFromContext(r.Context())
	if ok && authorization.OrgID != "" && isComplianceRole(authorization.UserRole) {
		return authorization, true
	}
	writeComplianceError(w, http.StatusForbidden, "compliance_role_required",
		"the audit compliance API is limited to organization owners and admins")
	return serviceAuthorization{}, false
}

func isComplianceRole(role string) bool {
	switch strings.ToLower(strings.TrimSpace(role)) {
	case "owner", "admin":
		return true
	default:
		return false
	}
}
