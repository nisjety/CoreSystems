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

// envComplianceRoleEnabled is the switch for the compliance organisation role
// (O-21). Only the literal "true" turns it on; unset, empty, "1" or "TRUE" leave
// the owner and admin lists and the 403 text exactly as they were.
const envComplianceRoleEnabled = "ORG_COMPLIANCE_ROLE_ENABLED"

func complianceRoleEnabled(getenv func(string) string) bool {
	return getenv(envComplianceRoleEnabled) == "true"
}

// requireComplianceRole returns the request's service authorization when its
// signed delegation names an owner or admin (or, with complianceOn, a compliance
// member), and otherwise writes the 403 itself. The role comes from the v3 delegation the gateway signs, not from
// anything the browser sent; audit-core checks it again even though the gateway
// gates the same routes, so a gateway bug cannot open the audit log to a member.
func requireComplianceRole(w http.ResponseWriter, r *http.Request, complianceOn bool) (serviceAuthorization, bool) {
	authorization, ok := serviceAuthorizationFromContext(r.Context())
	if ok && authorization.OrgID != "" && isAuditReaderRole(authorization.UserRole, complianceOn) {
		return authorization, true
	}
	message := "the audit compliance API is limited to organization owners and admins"
	if complianceOn {
		message = "the audit compliance API is limited to organization owners, admins and compliance members"
	}
	writeComplianceError(w, http.StatusForbidden, "compliance_role_required", message)
	return serviceAuthorization{}, false
}

// isAuditReaderRole is who may read the whole org audit trail and the compliance
// API: owner and admin always, and a compliance member only while the switch is
// on. Usage reads keep isComplianceRole (owner and admin) either way.
func isAuditReaderRole(role string, complianceOn bool) bool {
	if isComplianceRole(role) {
		return true
	}
	return complianceOn && strings.ToLower(strings.TrimSpace(role)) == "compliance"
}

func isComplianceRole(role string) bool {
	switch strings.ToLower(strings.TrimSpace(role)) {
	case "owner", "admin":
		return true
	default:
		return false
	}
}
