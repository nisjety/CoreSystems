package http

import (
	"context"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
)

// roleCapabilitiesEnvEnabled switches the member capability route on (ORG_ROLE_CAPABILITIES_ENABLED).
// Only the literal "true" turns it on. Off, the route answers 404 and no role is read.
const roleCapabilitiesEnvEnabled = "ORG_ROLE_CAPABILITIES_ENABLED"

// roleCapabilityTimeout bounds one read of the caller's role and its capabilities.
const roleCapabilityTimeout = 5 * time.Second

// roleCapabilitiesEnabled reads roleCapabilitiesEnvEnabled strictly, as orgFlagsEnabled does: a typo
// or "1" leaves the route unmounted.
func roleCapabilitiesEnabled(getenv func(string) string) bool {
	return getenv(roleCapabilitiesEnvEnabled) == "true"
}

// roleCapabilityReader is the seam the member capability route reads through. *rbac.Repository
// satisfies it; handler tests use a fake.
type roleCapabilityReader interface {
	EffectiveCapabilities(ctx context.Context, orgID, roleName string) ([]string, error)
}

// requireRoleCapabilitiesEnabled answers 404 on the member capability route until the switch is
// on, so a deployment that never enables it exposes nothing and reads no role.
func (s *Server) requireRoleCapabilitiesEnabled() gin.HandlerFunc {
	return func(c *gin.Context) {
		if !s.roleCapabilitiesOn {
			c.AbortWithStatusJSON(http.StatusNotFound, gin.H{"error": gin.H{"code": "not_found", "message": "not found"}})
			return
		}
		c.Next()
	}
}

// getMemberRoleCapabilities answers GET /orgs/:id/roles/:roleName/capabilities (org:read:self) with
// the effective capabilities of the role the signed-in member holds: {"capabilities":[...]}.
//
// It is the signed-user twin of getEffectiveCapabilities, the internal read that needs org:read:any
// and so can never be reached by verevon-gateway. The caller is the viewer of the verified
// delegation (x-user-id). The role comes from org-core's own membership table, never from the
// forwarded role, the same authority the other member reads use (WS10 D13). A roleName the caller
// does not hold is refused, so a member reads only the capability set of its own role. The
// capabilities are the same as the internal read's, including the base-tier defaults for a role
// with no stored row.
func (s *Server) getMemberRoleCapabilities(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	roleName := strings.TrimSpace(c.Param("roleName"))
	if orgID == "" || roleName == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": gin.H{"code": "invalid_request", "message": "organization id and role are required"}})
		return
	}
	callerID := strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": gin.H{"code": "unauthenticated", "message": "user not authenticated"}})
		return
	}
	if s.callerRoles == nil || s.roleCapabilities == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": gin.H{"code": "unavailable", "message": "role capabilities unavailable"}})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), roleCapabilityTimeout)
	defer cancel()

	role, err := s.callerRoles.CallerRole(ctx, orgID, callerID)
	if err != nil {
		log.Printf("role capabilities: caller role lookup failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": gin.H{"code": "internal_error", "message": "failed to resolve caller role"}})
		return
	}
	role = strings.TrimSpace(role)
	if role == "" {
		c.JSON(http.StatusForbidden, gin.H{"error": gin.H{"code": "forbidden", "message": "not a member of this organization"}})
		return
	}
	if role != roleName {
		c.JSON(http.StatusForbidden, gin.H{"error": gin.H{"code": "forbidden", "message": "role is not the caller's role"}})
		return
	}

	capabilities, err := s.roleCapabilities.EffectiveCapabilities(ctx, orgID, role)
	if err != nil {
		log.Printf("role capabilities: effective capabilities failed for org=%s role=%s: %v", orgID, role, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": gin.H{"code": "internal_error", "message": "failed to resolve effective capabilities"}})
		return
	}
	if capabilities == nil {
		capabilities = []string{}
	}
	c.JSON(http.StatusOK, gin.H{"capabilities": capabilities})
}
