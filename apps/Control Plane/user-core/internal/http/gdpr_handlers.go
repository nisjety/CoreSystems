package http

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// GDPR erasure + DSAR surface for users.
//
// These handlers wire the previously caller-less stored procedures
// (gdpr_hard_delete_user / gdpr_anonymize_user, defined in auth-core's
// migrations and executed against the auth_service DB) into callable,
// admin/self-gated, audited operations, plus a DSAR (Art. 15) export.
//
// Gating: every operation requires the caller to be EITHER a platform
// admin/superadmin (isAdminRequest) OR the data subject themselves (caller
// user id == target id). The irreversible hard-erasure additionally requires
// an explicit { "confirm": true } body flag.
//
// Reversible ban remains a separate action (DELETE /users/me is unchanged);
// erasure is the new irreversible hard path.

// resolveErasureActor returns the caller id + whether they are a platform admin,
// after confirming the caller may act on targetID. Writes 401/403 and returns
// ok=false when unauthorized.
func (s *Server) resolveErasureActor(c *gin.Context, targetID string) (callerID string, isAdmin bool, ok bool) {
	callerID, exists := getUserIDFromContext(c)
	if !exists {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return "", false, false
	}
	admin := isAdminRequest(c)
	if !admin && callerID != targetID {
		c.JSON(http.StatusForbidden, gin.H{"error": "admin role or self required"})
		return "", false, false
	}
	return callerID, admin, true
}

// actorRole returns a label for the audit actor_role field.
func actorRole(isAdmin, isSelf bool) string {
	switch {
	case isAdmin:
		return "admin"
	case isSelf:
		return "self"
	default:
		return "user"
	}
}

// auditOrgID resolves the org id to attach to the audit event. audit-core
// requires org_id; the gateway forwards the session's active org via X-Org-Id.
func auditOrgID(c *gin.Context) string {
	return strings.TrimSpace(c.GetHeader("X-Org-Id"))
}

// hardEraseUser irreversibly erases a user via gdpr_hard_delete_user (auth DB)
// plus local user_service cleanup. Admin or self; requires confirm: true.
// DELETE /api/v1/users/:id/gdpr/erase   Body: { "confirm": true }
func (s *Server) hardEraseUser(c *gin.Context) {
	targetID := strings.TrimSpace(c.Param("id"))
	if targetID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "user id is required"})
		return
	}

	var req struct {
		Confirm bool `json:"confirm"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	callerID, isAdmin, ok := s.resolveErasureActor(c, targetID)
	if !ok {
		return
	}

	if !req.Confirm {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "erasure is irreversible; set \"confirm\": true to proceed",
		})
		return
	}

	orgID := auditOrgID(c)
	role := actorRole(isAdmin, callerID == targetID)

	receipt, err := s.userService.HardEraseUser(c.Request.Context(), targetID)
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("gdpr hard erase failed")
		s.userService.PublishErasureAudit(orgID, "user", targetID, callerID, role, "error", nil)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to erase user"})
		return
	}

	s.userService.PublishErasureAudit(orgID, "user", targetID, callerID, role, "ok", receipt)
	c.JSON(http.StatusOK, receipt)
}

// anonymizeUser performs the softer GDPR variant via gdpr_anonymize_user
// (scrub PII + ban). Admin or self.
// POST /api/v1/users/:id/gdpr/anonymize
func (s *Server) anonymizeUser(c *gin.Context) {
	targetID := strings.TrimSpace(c.Param("id"))
	if targetID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "user id is required"})
		return
	}

	callerID, isAdmin, ok := s.resolveErasureActor(c, targetID)
	if !ok {
		return
	}

	orgID := auditOrgID(c)
	role := actorRole(isAdmin, callerID == targetID)

	receipt, err := s.userService.AnonymizeUser(c.Request.Context(), targetID)
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("gdpr anonymize failed")
		s.userService.PublishErasureAudit(orgID, "user_anonymize", targetID, callerID, role, "error", nil)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to anonymize user"})
		return
	}

	s.userService.PublishErasureAudit(orgID, "user_anonymize", targetID, callerID, role, "ok", receipt)
	c.JSON(http.StatusOK, receipt)
}

// dsarExport returns a GDPR Art. 15 data-subject export for the user. Admin or
// self. Audited (read), no erasure fan-out.
// GET /api/v1/users/:id/gdpr/export
func (s *Server) dsarExport(c *gin.Context) {
	targetID := strings.TrimSpace(c.Param("id"))
	if targetID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "user id is required"})
		return
	}

	callerID, isAdmin, ok := s.resolveErasureActor(c, targetID)
	if !ok {
		return
	}

	orgID := auditOrgID(c)
	role := actorRole(isAdmin, callerID == targetID)

	export, err := s.userService.BuildDSARExport(c.Request.Context(), targetID)
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("dsar export failed")
		s.userService.PublishDSARAudit(orgID, targetID, callerID, role, "error")
		c.JSON(http.StatusNotFound, gin.H{"error": "failed to build data export"})
		return
	}

	s.userService.PublishDSARAudit(orgID, targetID, callerID, role, "ok")
	c.JSON(http.StatusOK, export)
}
