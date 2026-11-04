package http

import (
	"context"
	"errors"
	"log"
	"net/http"
	"os"
	"strings"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/gin-gonic/gin"
)

// GDPR erasure surface for organizations.
//
// These handlers wire the previously caller-less stored procedures from
// migrations/003_gdpr_hard_delete.up.sql into callable, owner-gated, audited
// operations:
//
//   - gdpr_hard_delete_organization  (irreversible cascade delete)
//   - soft_delete_organization       (reversible; purged later by the cron)
//
// Every operation:
//   - is owner-gated (the caller must be an active "owner" of the org, OR a
//     platform admin/superadmin presented via X-User-Role);
//   - requires an explicit `confirm: true` body flag for the irreversible path;
//   - emits a durable audit event on verevon.audit.v2.control.org-core.erasure;
//   - emits a cross-plane fan-out on verevon.gdpr.erasure.requested so Model
//     Plane (run history / conversations) and Data Plane can purge their side.
//
// The org id is taken from the path and passed to the procs as a bound
// parameter ($1) — never string-interpolated.

const (
	// erasureAuditSubject is the durable audit subject consumed by audit-core
	// (verevon.audit.v2.control.<producer>.<event>).
	erasureAuditSubject = orgcore.GDPRErasureAuditSubject

	// gdprErasureFanoutSubject is the cross-plane erasure fan-out. Subscribers
	// (Model Plane run-history/conversations, Data Plane documents) consume
	// it from both the explicit immediate hard-delete path below AND the
	// 30-day retention cron (org.PurgeDeletedOrganizations).
	gdprErasureFanoutSubject = orgcore.GDPRErasureFanoutSubject
)

// errErasureNotConfirmed is returned when an irreversible erasure request is
// missing the explicit confirm flag.
const errErasureNotConfirmed = "erasure is irreversible; set \"confirm\": true to proceed"

// errOrgNameConfirmationMismatch is returned when the soft-delete request's
// org_name does not exactly match the organization's real, server-fetched
// name (the "type the org name to confirm" destructive-action pattern).
const errOrgNameConfirmationMismatch = "organization name confirmation does not match; type the exact organization name to confirm"

// Legal holds against erasure (EP-9 C4, decision D-C1). While any legal hold of
// the organisation is active, neither erasure route runs: an org hold covers
// everything, and a thread hold covers a thread the org purge would erase with
// the rest. The hold is released through the legal-hold routes first; there is
// no override here.
const (
	// envErasureHoldCheck is the switch. Only the literal "true" turns it on; off,
	// the erasure routes behave exactly as before.
	envErasureHoldCheck = "ERASURE_HOLD_CHECK"

	codeErasureBlockedLegalHold     = "erasure_blocked_legal_hold"
	codeErasureHoldCheckUnavailable = "erasure_hold_check_unavailable"
)

// heldErasureContent names what a hold keeps in Model Plane when it blocks an
// org erasure, matching session-core's held scope (thread goals and artifact
// revisions included, C-B9).
var heldErasureContent = []string{"messages", "runs", "plans", "approvals", "tasks", "agent_memory", "thread_goals", "artifact_revisions"}

// erasureHoldCheckEnabled reads the switch on every request, so a test or an
// operator flip needs no restart.
func erasureHoldCheckEnabled() bool {
	return os.Getenv(envErasureHoldCheck) == "true"
}

// refuseHeldErasure writes the refusal and reports true when orgID may not be
// erased now: 409 erasure_blocked_legal_hold while a hold is active, 503
// erasure_hold_check_unavailable when the holds cannot be read (a hold that
// cannot be ruled out is treated as present; the caller retries). Every refusal
// is logged with the actor; with the switch off it reads nothing.
func (s *Server) refuseHeldErasure(c *gin.Context, orgID, actorID, op string) bool {
	if !erasureHoldCheckEnabled() {
		return false
	}
	if s.retention == nil {
		log.Printf("gdpr: %s of org=%s by actor=%s refused: no retention service to read legal holds", op, orgID, actorID)
		writeRetentionError(c, http.StatusServiceUnavailable, codeErasureHoldCheckUnavailable,
			"Legal holds could not be checked, so nothing was erased. Try again shortly.", gin.H{"retryable": true})
		return true
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), retentionCheckTimeout)
	defer cancel()
	// Active holds are listed first, so the bounded list never hides one behind
	// released rows.
	list, err := s.retention.Holds(ctx, orgID)
	if err != nil {
		log.Printf("gdpr: %s of org=%s by actor=%s refused: legal holds unreadable: %v", op, orgID, actorID, err)
		writeRetentionError(c, http.StatusServiceUnavailable, codeErasureHoldCheckUnavailable,
			"Legal holds could not be checked, so nothing was erased. Try again shortly.", gin.H{"retryable": true})
		return true
	}
	orgHolds, threadHolds := 0, 0
	for _, hold := range list.Holds {
		switch {
		case !hold.Active():
		case hold.Scope == retention.ScopeOrg:
			orgHolds++
		default:
			threadHolds++
		}
	}
	if orgHolds+threadHolds == 0 {
		return false
	}
	log.Printf("gdpr: %s of org=%s by actor=%s refused: %d org and %d thread legal holds active", op, orgID, actorID, orgHolds, threadHolds)
	writeRetentionError(c, http.StatusConflict, codeErasureBlockedLegalHold,
		"A legal hold covers this organization, so nothing was erased. The hold must be released first.",
		gin.H{"org_holds": orgHolds, "thread_holds": threadHolds, "held_content": heldErasureContent, "retryable": false})
	return true
}

// confirmedErasure reports whether an irreversible erasure may proceed. Erasure
// is irreversible, so the caller MUST pass confirm:true. Extracted as a pure
// function so the guard is unit-testable without a database.
func confirmedErasure(confirm bool) bool {
	return confirm
}

// platformRoleIsAdmin reports whether the comma-separated role header carries
// a platform-level admin/superadmin role (forwarded by the gateway).
func platformRoleIsAdmin(role string) bool {
	for _, v := range strings.Split(role, ",") {
		switch strings.ToLower(strings.TrimSpace(v)) {
		case "admin", "superadmin":
			return true
		}
	}
	return false
}

// authorizeOrgErasure resolves the caller and ensures they may erase orgID.
// Returns (callerID, callerRole, true) when authorized; otherwise it writes the
// appropriate 401/403 response and returns ok=false.
func (s *Server) authorizeOrgErasure(c *gin.Context, orgID string) (string, string, bool) {
	callerID := strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "user not authenticated"})
		return "", "", false
	}

	// Platform admins (gateway-forwarded role) may erase any org.
	platformRole := strings.TrimSpace(c.GetHeader("X-User-Role"))
	if platformRole == "" {
		platformRole = strings.TrimSpace(c.GetHeader("X-User-Roles"))
	}
	if platformRoleIsAdmin(platformRole) {
		return callerID, "platform:" + platformRole, true
	}

	// Otherwise the caller must be an active OWNER of the target org.
	role, err := s.orgService.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve caller role"})
		return "", "", false
	}
	if strings.ToLower(strings.TrimSpace(role)) != "owner" {
		c.JSON(http.StatusForbidden, gin.H{"error": "organization owner role required"})
		return "", "", false
	}
	return callerID, role, true
}

// publishErasureFanout emits the independent cross-plane erasure contract.
// The local audit event is already committed atomically with the erasure and
// is delivered by the durable outbox worker. Delegates to the Service so the
// exact same publish logic also runs from the cron-triggered
// PurgeDeletedOrganizations path (internal/org/service_enhanced.go) — both
// paths must emit this fan-out identically.
func (s *Server) publishErasureFanout(orgID, actorID string) {
	s.orgService.PublishGDPRErasureFanout(orgID, actorID)
}

// hardDeleteOrganization erases an organization and all its data via
// gdpr_hard_delete_organization. Irreversible — requires confirm: true.
// DELETE /orgs/:id/gdpr/erase   Body: { "confirm": true }
func (s *Server) hardDeleteOrganization(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}

	var req struct {
		Confirm bool `json:"confirm"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	actorID, actorRole, ok := s.authorizeOrgErasure(c, orgID)
	if !ok {
		return
	}

	if !confirmedErasure(req.Confirm) {
		c.JSON(http.StatusBadRequest, gin.H{"error": errErasureNotConfirmed})
		return
	}

	if s.refuseHeldErasure(c, orgID, actorID, "hard delete") {
		return
	}

	receipt, err := s.orgService.HardDelete(c.Request.Context(), orgID, actorID, actorRole)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to erase organization"})
		return
	}

	s.publishErasureFanout(orgID, actorID)
	c.Data(http.StatusOK, "application/json", receipt)
}

// softDeleteOrganization marks an organization deleted (reversible until the
// retention cron purges it) via soft_delete_organization. Opens the 30-day
// Flow C grace window: creates one org_deletion_members ledger row per
// active member and publishes verevon.org.deletion.pending.
// DELETE /orgs/:id/gdpr/soft-delete   Body: { "confirm": true, "org_name": "<exact org name>" }
func (s *Server) softDeleteOrganization(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}

	var req struct {
		Confirm bool   `json:"confirm"`
		OrgName string `json:"org_name"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	actorID, actorRole, ok := s.authorizeOrgErasure(c, orgID)
	if !ok {
		return
	}

	if !confirmedErasure(req.Confirm) {
		c.JSON(http.StatusBadRequest, gin.H{"error": errErasureNotConfirmed})
		return
	}

	// A soft delete starts the clock of the automatic purge, so a held org may
	// not enter it either.
	if s.refuseHeldErasure(c, orgID, actorID, "soft delete") {
		return
	}

	receipt, err := s.orgService.SoftDelete(c.Request.Context(), orgID, req.OrgName, actorID, actorRole)
	if err != nil {
		var nameMismatch *orgcore.ErrOrgNameMismatch
		if errors.As(err, &nameMismatch) {
			c.JSON(http.StatusBadRequest, gin.H{"error": errOrgNameConfirmationMismatch})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to soft-delete organization"})
		return
	}

	// Soft delete is reversible, so it does not emit the irreversible
	// cross-plane erasure fan-out. Its audit intent was committed atomically
	// by the service; the deletion ledger + pending notice were handled by
	// Service.SoftDelete itself.
	c.Data(http.StatusOK, "application/json", receipt)
}

// restoreOrganization reverses a pending soft-delete: clears deleted_at back
// to active, wipes the organization's deletion ledger, and publishes
// verevon.org.deletion.cancelled. Owner-gated, same as the two erasure routes
// above. 409 if the organization is not currently pending deletion.
// POST /orgs/:id/gdpr/restore
func (s *Server) restoreOrganization(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}

	actorID, actorRole, ok := s.authorizeOrgErasure(c, orgID)
	if !ok {
		return
	}

	if err := s.orgService.RestoreOrganization(c.Request.Context(), orgID, actorID, actorRole); err != nil {
		if errors.Is(err, orgcore.ErrOrganizationNotPendingDeletion) {
			c.JSON(http.StatusConflict, gin.H{"error": "organization is not pending deletion"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to restore organization"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"ok": true})
}

// authorizeDeletionSelfService resolves the caller for the lower-stakes Flow
// C self-service surface (mark-exported / acknowledge / status). Unlike
// authorizeOrgErasure (owner-only), ANY ACTIVE MEMBER may act on their own
// ledger row, and a platform admin may act regardless of membership —
// mirroring user-core's resolveErasureActor self-or-platform-admin pattern,
// adapted to org-core's caller-role model since org-core has no equivalent
// shared helper. isPrivileged reports whether the caller is a platform admin
// OR an org-level owner/admin — the status handler uses it to decide whether
// to include every member's ledger row.
func (s *Server) authorizeDeletionSelfService(c *gin.Context, orgID string) (callerID string, isPrivileged bool, ok bool) {
	callerID = strings.TrimSpace(c.GetHeader("x-user-id"))
	if callerID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "user not authenticated"})
		return "", false, false
	}

	platformRole := strings.TrimSpace(c.GetHeader("X-User-Role"))
	if platformRole == "" {
		platformRole = strings.TrimSpace(c.GetHeader("X-User-Roles"))
	}
	if platformRoleIsAdmin(platformRole) {
		return callerID, true, true
	}

	orgRole, err := s.orgService.CallerRole(c.Request.Context(), orgID, callerID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve caller role"})
		return "", false, false
	}
	orgRole = strings.ToLower(strings.TrimSpace(orgRole))
	if orgRole == "" {
		c.JSON(http.StatusForbidden, gin.H{"error": "organization membership required"})
		return "", false, false
	}
	return callerID, orgRole == "owner" || orgRole == "admin", true
}

// markExported records that the calling member has received their
// personal-data export ahead of the organization's scheduled purge.
// POST /orgs/:id/gdpr/deletion/mark-exported   Body: none.
func (s *Server) markExported(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, _, ok := s.authorizeDeletionSelfService(c, orgID)
	if !ok {
		return
	}
	if err := s.orgService.MarkDeletionExported(c.Request.Context(), orgID, callerID); err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "no pending-deletion record for this member"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to record export acknowledgement"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

// acknowledgeDeletion records that the calling member has acknowledged the
// organization's pending deletion notice.
// POST /orgs/:id/gdpr/deletion/acknowledge   Body: none.
func (s *Server) acknowledgeDeletion(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, _, ok := s.authorizeDeletionSelfService(c, orgID)
	if !ok {
		return
	}
	if err := s.orgService.MarkDeletionAcknowledged(c.Request.Context(), orgID, callerID); err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "no pending-deletion record for this member"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to record deletion acknowledgement"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

// getDeletionStatus reports the organization's pending-deletion window (if
// any) and the calling member's own export/acknowledge checkpoints. An
// owner/admin caller (org-level or platform) additionally receives every
// member's ledger row.
// GET /orgs/:id/gdpr/deletion/status
func (s *Server) getDeletionStatus(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, isPrivileged, ok := s.authorizeDeletionSelfService(c, orgID)
	if !ok {
		return
	}
	status, err := s.orgService.GetDeletionStatus(c.Request.Context(), orgID, callerID, isPrivileged)
	if err != nil {
		if errors.Is(err, orgcore.ErrNotFound) {
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read deletion status"})
		return
	}
	c.JSON(http.StatusOK, status)
}

// erasureFanoutSubjects exposes the fan-out + audit subjects for documentation
// and tests (so the contract can be asserted without hitting NATS).
func erasureFanoutSubjects() (audit, fanout string) {
	return erasureAuditSubject, gdprErasureFanoutSubject
}
