package org

// EnvComplianceRoleEnabled is the switch for the compliance organisation role
// (O-21). Only the literal "true" turns it on. Off, "compliance" is refused as a
// role value in every org-core role check, as before.
const EnvComplianceRoleEnabled = "ORG_COMPLIANCE_ROLE_ENABLED"

// RoleCompliance is the organisation role that reads and places legal holds and
// reads audit, and runs no agents.
const RoleCompliance = "compliance"

// ComplianceRoleEnabled reads EnvComplianceRoleEnabled strictly: only the literal
// "true" turns the role on, so a typo or "1" leaves it refused.
func ComplianceRoleEnabled(getenv func(string) string) bool {
	return getenv(EnvComplianceRoleEnabled) == "true"
}

// IsAssignableMemberRole reports whether role may be written as a member's
// organisation role. The four base tiers always may; compliance only while the
// switch is on. The match is exact, as the callers already lower-case or compare
// the raw value.
func IsAssignableMemberRole(role string, complianceOn bool) bool {
	switch role {
	case "owner", "admin", "member", "viewer":
		return true
	case RoleCompliance:
		return complianceOn
	}
	return false
}
