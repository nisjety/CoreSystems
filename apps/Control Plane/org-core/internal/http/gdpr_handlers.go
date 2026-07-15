package http

import (
	"net/http"
	"strings"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
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
//   - emits a durable audit event on velion.audit.v2.control.org-core.erasure;
//   - emits a cross-plane fan-out on velion.gdpr.erasure.requested so Model
//     Plane (run history / conversations) and Data Plane can purge their side.
//
// The org id is taken from the path and passed to the procs as a bound
// parameter ($1) — never string-interpolated.

const (
	// erasureAuditSubject is the durable audit subject consumed by audit-core
	// (velion.audit.v2.control.<producer>.<event>).
	erasureAuditSubject = orgcore.GDPRErasureAuditSubject

	// gdprErasureFanoutSubject is the cross-plane erasure fan-out. Subscribers
	// (Model Plane run-history/conversations, Data Plane documents) are a
	// documented follow-up — org-core only emits the contract today.
	gdprErasureFanoutSubject = "velion.gdpr.erasure.requested"
)

// errErasureNotConfirmed is returned when an irreversible erasure request is
// missing the explicit confirm flag.
const errErasureNotConfirmed = "erasure is irreversible; set \"confirm\": true to proceed"

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
// is delivered by the durable outbox worker.
func (s *Server) publishErasureFanout(orgID, actorID string) {
	if sp := s.orgService.SharedPub(); sp != nil {
		sp.PublishPlain(gdprErasureFanoutSubject, map[string]any{
			"subject_type": "organization",
			"subject_id":   orgID,
			"org_id":       orgID,
			"requested_by": actorID,
			"ts":           time.Now().UTC().Format(time.RFC3339Nano),
		})
	}
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

	receipt, err := s.orgService.HardDelete(c.Request.Context(), orgID, actorID, actorRole)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to erase organization"})
		return
	}

	s.publishErasureFanout(orgID, actorID)
	c.Data(http.StatusOK, "application/json", receipt)
}

// softDeleteOrganization marks an organization deleted (reversible until the
// retention cron purges it) via soft_delete_organization.
// DELETE /orgs/:id/gdpr/soft-delete
func (s *Server) softDeleteOrganization(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}

	actorID, actorRole, ok := s.authorizeOrgErasure(c, orgID)
	if !ok {
		return
	}

	receipt, err := s.orgService.SoftDelete(c.Request.Context(), orgID, actorID, actorRole)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to soft-delete organization"})
		return
	}

	// Soft delete is reversible, so it does not emit the cross-plane purge
	// fan-out. Its audit intent was committed atomically by the service.
	c.Data(http.StatusOK, "application/json", receipt)
}

// erasureFanoutSubjects exposes the fan-out + audit subjects for documentation
// and tests (so the contract can be asserted without hitting NATS).
func erasureFanoutSubjects() (audit, fanout string) {
	return erasureAuditSubject, gdprErasureFanoutSubject
}
