package http

import (
	"errors"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// W3-DEC-ORGAGENT (owner decision D19): the org-admin routes of the org-visible agent
// definitions, under /api/v1/internal/agents/org-subjects. The gateway forwards an org admin's
// request with a signed delegation. This layer re-resolves the canonical org role from auth-core
// on every call (the check the Space policy routes use) and admits only an owner or admin.
// Every route answers the typed error envelope {"error":{"code","message","details"}}.

// orgAgentAdminRoleKey carries the verified canonical role to the handler (audit actor_role).
const orgAgentAdminRoleKey = "org_agent_admin_role"

// requireOrgAgentAdmin admits only the gateway workload with a verified delegation, the
// agents:manage scope, and a canonical owner or admin role of the delegated organization.
func (s *Server) requireOrgAgentAdmin() gin.HandlerFunc {
	return func(c *gin.Context) {
		userID, orgID := strings.TrimSpace(c.GetString("user_id")), strings.TrimSpace(c.GetString("org_id"))
		if c.GetString("auth_method") != "service_principal" ||
			c.GetString("service_id") != verevonGatewayPrincipal ||
			!c.GetBool("delegation_verified") ||
			!hasServiceScope(c, agentsManageScope) ||
			userID == "" || orgID == "" {
			abortTypedError(c, http.StatusForbidden, "space_org_admin_required",
				"a signed gateway delegation for an organization admin is required", nil)
			return
		}
		if s.spaceRepo == nil {
			writeAgentError(c, errors.New("agent registry repository unavailable"))
			return
		}
		role, err := s.orgAdminRole(c.Request.Context(), userID, orgID)
		if errors.Is(err, ErrMembershipNotFound) {
			abortTypedError(c, http.StatusForbidden, "space_org_membership_required",
				"organization membership is required", nil)
			return
		}
		if err != nil {
			log.Error().Err(err).Msg("org agent admin: canonical membership authority unavailable")
			abortTypedError(c, http.StatusServiceUnavailable, "space_policy_membership_authority_unavailable",
				"organization membership could not be verified", nil)
			return
		}
		if role != "owner" && role != "admin" {
			abortTypedError(c, http.StatusForbidden, "space_org_admin_required",
				"an organization owner or admin is required", nil)
			return
		}
		c.Set(orgAgentAdminRoleKey, role)
		c.Next()
	}
}

// listOrgAgentSubjects lists the org-visible agent definitions of the delegated organization
// with their lifecycle, for the org-admin screen.
func (s *Server) listOrgAgentSubjects(c *gin.Context) {
	agents, err := s.spaceRepo.ListOrgAgents(c.Request.Context(), c.GetString("org_id"))
	if err != nil {
		writeAgentError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"agents": agents}})
}

// orgAgentLifecycleBody carries the revision the admin read. It is required on all three
// routes, so a stale screen cannot change an agent it no longer shows.
type orgAgentLifecycleBody struct {
	ExpectedRevision *int64 `json:"expected_revision"`
}

// changeOrgAgentLifecycle applies one org-admin action at the revision the admin read. The
// result reports the lifecycle after the change, whether it changed, and how many rooms moved.
func (s *Server) changeOrgAgentLifecycle(c *gin.Context, action string) {
	var body orgAgentLifecycleBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	if body.ExpectedRevision == nil {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest,
			"expected_revision is required", map[string]any{"field": "expected_revision"})
		return
	}
	result, err := s.spaceRepo.ChangeOrgAgentLifecycle(c.Request.Context(), spaces.OrgAgentLifecycleRequest{
		OrgID:            c.GetString("org_id"),
		ActorID:          c.GetString("user_id"),
		ActorOrgRole:     c.GetString(orgAgentAdminRoleKey),
		AgentSubjectID:   c.Param("agent_subject_id"),
		Action:           action,
		ExpectedRevision: *body.ExpectedRevision,
	})
	if err != nil {
		writeAgentError(c, err)
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"agent_subject_id": result.AgentSubjectID,
		"lifecycle":        result.Lifecycle,
		"revision":         result.Revision,
		"changed":          result.Changed,
		"rooms_changed":    result.RoomsChanged,
	}})
}

// suspendOrgAgentSubject suspends an active org-visible agent, its admissions and its seats.
func (s *Server) suspendOrgAgentSubject(c *gin.Context) {
	s.changeOrgAgentLifecycle(c, spaces.OrgAgentActionSuspend)
}

// reactivateOrgAgentSubject restores a suspended org-visible agent. Rooms admit it again.
func (s *Server) reactivateOrgAgentSubject(c *gin.Context) {
	s.changeOrgAgentLifecycle(c, spaces.OrgAgentActionReactivate)
}

// retireOrgAgentSubject retires an org-visible agent (soft delete): its admissions are
// withdrawn in every room and its seats removed. The row stays for audit.
func (s *Server) retireOrgAgentSubject(c *gin.Context) {
	s.changeOrgAgentLifecycle(c, spaces.OrgAgentActionRetire)
}
