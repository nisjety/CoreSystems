package http

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

const maxGDPROperatorBodyBytes = 16 * 1024

type gdprRequeueRequest struct {
	Kind     string   `json:"kind"`
	EventIDs []string `json:"event_ids"`
}

func decodeStrictGDPRJSON(c *gin.Context, destination any) error {
	c.Request.Body = http.MaxBytesReader(c.Writer, c.Request.Body, maxGDPROperatorBodyBytes)
	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(destination); err != nil {
		return err
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		if err == nil {
			return io.ErrUnexpectedEOF
		}
		return err
	}
	return nil
}

func (s *Server) requeueGDPRDeliveries(c *gin.Context) {
	if _, authenticated := getUserIDFromContext(c); !authenticated {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return
	}
	if !isAdminRequest(c) {
		c.JSON(http.StatusForbidden, gin.H{"error": "admin role required"})
		return
	}
	var request gdprRequeueRequest
	if err := decodeStrictGDPRJSON(c, &request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	request.Kind = strings.TrimSpace(request.Kind)
	if (request.Kind != "audit" && request.Kind != "fanout") || len(request.EventIDs) == 0 || len(request.EventIDs) > 100 {
		c.JSON(http.StatusBadRequest, gin.H{"error": "kind must be audit or fanout and event_ids must contain 1-100 values"})
		return
	}
	requeued, err := s.userService.RequeueGDPRDeliveries(c.Request.Context(), request.Kind, request.EventIDs)
	if err != nil {
		log.Error().Err(err).Msg("GDPR delivery requeue failed")
		c.JSON(http.StatusBadRequest, gin.H{"error": "delivery requeue rejected"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"requeued": requeued, "kind": request.Kind}})
}

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

// verifiedAuditOrgHint returns an org only when service-auth middleware has
// cryptographically verified a delegated user+org binding. Raw X-Org-Id and
// unverified context values are deliberately ignored.
func verifiedAuditOrgHint(c *gin.Context) string {
	if c.GetString("auth_method") != "service_principal" || !c.GetBool("delegation_verified") {
		return ""
	}
	return strings.TrimSpace(c.GetString("org_id"))
}

// writeSuccessionError maps the admin-succession pre-flight gate's structured
// errors (EnsureSuccession, gdpr_succession.go) to the project's
// { "error": { "code", "message", "details" } } envelope and writes a 409.
// Returns false (writes nothing) for any other error, so the caller can fall
// back to its own generic failure response.
func writeSuccessionError(c *gin.Context, err error) bool {
	var required *users.ErrSuccessorRequired
	if errors.As(err, &required) {
		c.JSON(http.StatusConflict, gin.H{"error": gin.H{
			"code":    "successor_required",
			"message": "this user is the sole owner or admin of one or more organizations; a successor_user_id must be supplied before erasure can proceed",
			"details": gin.H{"orgs": required.Orgs},
		}})
		return true
	}
	var invalid *users.ErrSuccessorInvalid
	if errors.As(err, &invalid) {
		c.JSON(http.StatusConflict, gin.H{"error": gin.H{
			"code":    "successor_invalid",
			"message": invalid.Error(),
			"details": gin.H{"org_id": invalid.OrgID},
		}})
		return true
	}
	return false
}

// erasureUnavailable refuses an erasure/anonymize request with an explicit 503
// when the auth-DB pool is not wired (AUTH_DATABASE_URL unset). This replaces the
// previous opaque 500 — an Art. 17 trap where the route looked live but always
// failed. DSAR export is independent and stays available.
func erasureUnavailable(c *gin.Context) {
	c.JSON(http.StatusServiceUnavailable, gin.H{
		"error":   "erasure_unavailable",
		"message": "User erasure is not configured on this deployment (AUTH_DATABASE_URL unset). Data export (DSAR) is unaffected.",
	})
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
		Confirm         bool   `json:"confirm"`
		SuccessorUserID string `json:"successor_user_id"`
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

	// Capability gate after authz + confirm: if the auth-DB pool is not wired
	// (AUTH_DATABASE_URL unset) refuse with an explicit 503 instead of attempting
	// the proc and returning an opaque 500.
	if !s.userService.ErasureAvailable() {
		erasureUnavailable(c)
		return
	}

	// Admin-succession pre-flight: a sole owner/admin of one or more
	// organizations must not self-erase (or be erased) without first handing
	// off to a validated successor. Runs BEFORE the erasure saga is touched.
	if err := s.userService.EnsureSuccession(c.Request.Context(), targetID, req.SuccessorUserID); err != nil {
		if writeSuccessionError(c, err) {
			return
		}
		log.Error().Err(err).Str("subject_id", targetID).Msg("admin-succession pre-flight failed")
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "succession_unavailable"})
		return
	}

	orgID, err := s.userService.ResolveErasureAuditOrg(c.Request.Context(), targetID, verifiedAuditOrgHint(c))
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("failed to derive erasure audit scope")
		c.JSON(http.StatusConflict, gin.H{"error": "audit_scope_unavailable"})
		return
	}
	role := actorRole(isAdmin, callerID == targetID)

	receipt, err := s.userService.HardEraseUser(c.Request.Context(), targetID, callerID, role, orgID)
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("gdpr hard erase retained for retry")
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "erasure_pending", "operation": receipt})
		return
	}

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

	var req struct {
		SuccessorUserID string `json:"successor_user_id"`
	}
	if c.Request.ContentLength != 0 {
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
			return
		}
	}

	callerID, isAdmin, ok := s.resolveErasureActor(c, targetID)
	if !ok {
		return
	}

	if !s.userService.ErasureAvailable() {
		erasureUnavailable(c)
		return
	}

	// Admin-succession pre-flight: see hardEraseUser for the full rationale.
	if err := s.userService.EnsureSuccession(c.Request.Context(), targetID, req.SuccessorUserID); err != nil {
		if writeSuccessionError(c, err) {
			return
		}
		log.Error().Err(err).Str("subject_id", targetID).Msg("admin-succession pre-flight failed")
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "succession_unavailable"})
		return
	}

	orgID, err := s.userService.ResolveErasureAuditOrg(c.Request.Context(), targetID, verifiedAuditOrgHint(c))
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("failed to derive anonymize audit scope")
		c.JSON(http.StatusConflict, gin.H{"error": "audit_scope_unavailable"})
		return
	}
	role := actorRole(isAdmin, callerID == targetID)

	receipt, err := s.userService.AnonymizeUser(c.Request.Context(), targetID, callerID, role, orgID)
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("gdpr anonymize retained for retry")
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "erasure_pending", "operation": receipt})
		return
	}
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

	orgID, err := s.userService.ResolveErasureAuditOrg(c.Request.Context(), targetID, verifiedAuditOrgHint(c))
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("failed to derive DSAR audit scope")
		c.JSON(http.StatusConflict, gin.H{"error": "audit_scope_unavailable"})
		return
	}
	role := actorRole(isAdmin, callerID == targetID)

	export, err := s.userService.BuildDSARExport(c.Request.Context(), targetID)
	if err != nil {
		log.Error().Err(err).Str("subject_id", targetID).Msg("dsar export failed")
		if auditErr := s.userService.PublishDSARAudit(c.Request.Context(), orgID, targetID, callerID, role, "error"); auditErr != nil {
			log.Error().Err(auditErr).Msg("failed to persist DSAR failure audit")
		}
		c.JSON(http.StatusNotFound, gin.H{"error": "failed to build data export"})
		return
	}

	if auditErr := s.userService.PublishDSARAudit(c.Request.Context(), orgID, targetID, callerID, role, "ok"); auditErr != nil {
		log.Error().Err(auditErr).Msg("DSAR audit intent was not persisted")
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "audit_persistence_failed"})
		return
	}
	c.JSON(http.StatusOK, export)
}
