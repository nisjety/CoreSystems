package http

import (
	"errors"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S5a: the gateway-delegated surface of the Space tool policy. The body
// carries no actor, organisation, role or entitlement: the verified delegation
// names the person, and Control resolves the rest. A policy only tightens
// (allow < ask < block), so writing one needs a current manager or owner and no
// entitlement, and it is allowed under Zero Data Retention (keys and levels,
// never content). Reading is open to any member who can read the Space.

func writeToolPolicyError(c *gin.Context, err error) {
	switch {
	case errors.Is(err, spaces.ErrToolPolicyConflict):
		abortTypedError(c, http.StatusConflict, "tool_policy_revision_conflict", "The tool policy changed; reload and retry", nil)
		return
	case errors.Is(err, spaces.ErrApprovalAgentNotInSpace):
		abortTypedError(c, http.StatusConflict, "approval_agent_not_in_space", "The agent is not an active member of this Space", nil)
		return
	}
	writeApprovalError(c, err)
}

// toolPolicyManager resolves the caller's CURRENT membership (an archived room
// is read-only) and answers the typed refusal when it is missing or below
// manager. A personal Space has no shared agents and no tool policy.
func (s *Server) toolPolicyManager(c *gin.Context, spaceRef string) (*spaces.CurrentMembership, bool) {
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(
		c.Request.Context(), spaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		writeApprovalError(c, err)
		return nil, false
	}
	if !spaces.IsElevatedRole(membership.Role) {
		abortTypedError(c, http.StatusForbidden, "tool_policy_role_required", "Only a Space manager can set the tool policy",
			map[string]any{"required_roles": []string{spaces.RoleManager, spaces.RoleOwner}})
		return nil, false
	}
	if membership.Kind == spaces.KindPersonal {
		abortTypedError(c, http.StatusForbidden, "space_kind_unsupported", "A personal Space has no tool policy", nil)
		return nil, false
	}
	return membership, true
}

// getToolPolicy returns the Space ceiling, every stored agent layer and, when
// agent_subject_id is named, the merged view for that agent (the level and the
// layer that decided it, per tool) with its digest.
func (s *Server) getToolPolicy(c *gin.Context) {
	if !s.approvalRepoAvailable(c) {
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	membership, err := s.spaceRepo.ResolveReadableUserMembership(
		c.Request.Context(), spaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	agentID := strings.TrimSpace(c.Query("agent_subject_id"))
	if len(agentID) > 200 {
		abortTypedError(c, http.StatusBadRequest, "tool_policy_invalid", "agent_subject_id is oversized", nil)
		return
	}
	ceiling, err := s.spaceRepo.GetToolPolicySet(c.Request.Context(), spaceRef, membership.OrgID, spaces.ToolPolicyLayerSpaceCeiling, "")
	if err != nil {
		writeToolPolicyError(c, err)
		return
	}
	agents, err := s.spaceRepo.ListAgentToolPolicySets(c.Request.Context(), spaceRef, membership.OrgID)
	if err != nil {
		writeToolPolicyError(c, err)
		return
	}
	var agentLayer *spaces.ToolPolicySet
	if agentID != "" {
		set, err := s.spaceRepo.GetToolPolicySet(c.Request.Context(), spaceRef, membership.OrgID, spaces.ToolPolicyLayerAgent, agentID)
		if err != nil {
			writeToolPolicyError(c, err)
			return
		}
		agentLayer = &set
	}
	merged := spaces.MergeToolPolicy(&ceiling, agentLayer)
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref": spaceRef,
		"ceiling":   ceiling,
		"agents":    agents,
		"merged":    merged,
	}})
}

func (s *Server) putSpaceToolPolicy(c *gin.Context) {
	s.putToolPolicyLayer(c, spaces.ToolPolicyLayerSpaceCeiling, "")
}

func (s *Server) putAgentToolPolicy(c *gin.Context) {
	s.putToolPolicyLayer(c, spaces.ToolPolicyLayerAgent, strings.TrimSpace(c.Param("agent_subject_id")))
}

// putToolPolicyLayer replaces one layer under a revision compare-and-set. The
// actor is the verified user; the Space and organisation come from the resolved
// membership.
func (s *Server) putToolPolicyLayer(c *gin.Context, layer, agentSubjectID string) {
	if !s.approvalRepoAvailable(c) {
		return
	}
	var update spaces.ToolPolicyUpdate
	if !decodeApprovalBody(c, &update) {
		return
	}
	if err := update.Validate(); err != nil {
		abortTypedError(c, http.StatusBadRequest, "tool_policy_invalid", err.Error(), nil)
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	membership, ok := s.toolPolicyManager(c, spaceRef)
	if !ok {
		return
	}
	set, err := s.spaceRepo.ReplaceToolPolicySet(c.Request.Context(), spaceRef, membership.OrgID,
		c.GetString("user_id"), layer, agentSubjectID, update)
	if err != nil {
		writeToolPolicyError(c, err)
		return
	}
	var agentLayer *spaces.ToolPolicySet
	ceiling := set
	if layer == spaces.ToolPolicyLayerAgent {
		current, err := s.spaceRepo.GetToolPolicySet(c.Request.Context(), spaceRef, membership.OrgID, spaces.ToolPolicyLayerSpaceCeiling, "")
		if err != nil {
			writeToolPolicyError(c, err)
			return
		}
		ceiling = current
		agentLayer = &set
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"policy": set,
		"merged": spaces.MergeToolPolicy(&ceiling, agentLayer),
	}})
}
