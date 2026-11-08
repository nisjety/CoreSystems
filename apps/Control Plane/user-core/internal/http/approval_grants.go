package http

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S2: the gateway-delegated surface of Control-held approval authority.
// The body carries no actor, organisation, role, revision or entitlement: the
// verified delegation names the person, and Control resolves the rest.
//
// Standing rules are limited to scope "always", to MCP tools, and to a Space
// whose ceiling allows them; creating one needs manager or owner, the
// approval_grant entitlement and a non-ZDR Space. Listing and revoking need no
// entitlement: tightening must keep working when the entitlement is switched off.

const maxApprovalBodyBytes = 16 * 1024

// writeApprovalError maps a repository or evidence error to its typed refusal.
// Nothing here echoes attestation values, tokens or content.
func writeApprovalError(c *gin.Context, err error) {
	switch {
	case errors.Is(err, spaces.ErrNoCurrentMembership):
		abortTypedError(c, http.StatusNotFound, "space_not_found", "Space not found", nil)
		return
	case errors.Is(err, spaces.ErrApprovalScopeNotPermitted):
		abortTypedError(c, http.StatusForbidden, "approval_scope_not_permitted", "This approval scope is not permitted here", nil)
		return
	case errors.Is(err, spaces.ErrApprovalRuleNotFound):
		abortTypedError(c, http.StatusNotFound, "approval_grant_not_found", "Approval rule not found", nil)
		return
	case errors.Is(err, spaces.ErrApprovalAgentNotInSpace):
		abortTypedError(c, http.StatusConflict, "approval_agent_not_in_space", "The agent is not an active member of this Space", nil)
		return
	case errors.Is(err, spaces.ErrApprovalPolicyConflict):
		abortTypedError(c, http.StatusConflict, "approval_policy_revision_conflict", "The approval policy changed; reload and retry", nil)
		return
	case errors.Is(err, spaces.ErrApprovalIdempotencyReused):
		abortTypedError(c, http.StatusConflict, "idempotency_key_reused", "The idempotency key was used for a different approval rule", nil)
		return
	}
	if denial, ok := spaces.AsDenial(err); ok {
		switch denial.Code {
		case spaces.DenialEntitlementOff:
			abortTypedError(c, http.StatusForbidden, "approval_grants_not_entitled", "Standing approval rules are not enabled for this organization", denial.Details())
		case spaces.DenialRoleBelowFloor:
			abortTypedError(c, http.StatusForbidden, "approval_scope_not_permitted", "Your Space role cannot use this approval scope", denial.Details())
		case spaces.DenialZDRForbidsEffect:
			abortTypedError(c, http.StatusPreconditionFailed, "zdr_durable_mutation_forbidden", "Standing approval rules are not available under Zero Data Retention", denial.Details())
		default:
			abortTypedError(c, denial.Status(), string(denial.Code), "Space authority does not allow this", denial.Details())
		}
		return
	}
	abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval authority is unavailable", nil)
}

// decodeApprovalBody decodes a strict JSON body, answering 400 on any problem.
func decodeApprovalBody(c *gin.Context, into any) bool {
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, maxApprovalBodyBytes))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(into); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		abortTypedError(c, http.StatusBadRequest, "invalid_request", "A valid request body is required", nil)
		return false
	}
	return true
}

func (s *Server) approvalRepoAvailable(c *gin.Context) bool {
	if s.spaceRepo == nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval authority is unavailable", nil)
		return false
	}
	return true
}

type createApprovalGrantRequest struct {
	SpaceRef         string `json:"space_ref"`
	Scope            string `json:"scope"`
	ApprovalID       string `json:"approval_id"`
	AgentSubjectID   string `json:"agent_subject_id"`
	ToolKey          string `json:"tool_key"`
	ActionKey        string `json:"action_key"`
	ActionSchemaHash string `json:"action_schema_hash"`
	IdempotencyKey   string `json:"idempotency_key"`
}

// createApprovalGrant records one standing rule. approval_id is optional
// provenance only (Control does not yet verify it against Session Core).
func (s *Server) createApprovalGrant(c *gin.Context) {
	if !s.approvalRepoAvailable(c) {
		return
	}
	var body createApprovalGrantRequest
	if !decodeApprovalBody(c, &body) {
		return
	}
	// Control stores standing rules (always) only; once and thread are Session Core's.
	if body.Scope != "" && body.Scope != string(spaces.ApprovalScopeAlways) {
		writeApprovalError(c, spaces.ErrApprovalScopeNotPermitted)
		return
	}
	request := spaces.ApprovalRuleRequest{
		SpaceRef: body.SpaceRef, AgentSubjectID: body.AgentSubjectID, ToolKey: body.ToolKey,
		ActionKey: body.ActionKey, ActionSchemaHash: body.ActionSchemaHash,
		SourceApprovalID: body.ApprovalID, IdempotencyKey: body.IdempotencyKey,
	}
	if err := request.Validate(); err != nil {
		if errors.Is(err, spaces.ErrApprovalScopeNotPermitted) {
			writeApprovalError(c, err)
			return
		}
		abortTypedError(c, http.StatusBadRequest, "invalid_request", err.Error(), nil)
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedApprovalDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	rule, _, err := s.spaceRepo.CreateApprovalRule(c.Request.Context(), evidence, request, time.Now().UTC())
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"grant": rule}})
}

// elevatedMembership resolves the caller's readable membership and answers the
// typed refusal when it is missing or below manager.
func (s *Server) elevatedMembership(c *gin.Context, spaceRef string) (*spaces.CurrentMembership, bool) {
	membership, err := s.spaceRepo.ResolveReadableUserMembership(
		c.Request.Context(), spaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		writeApprovalError(c, err)
		return nil, false
	}
	if !spaces.IsElevatedRole(membership.Role) {
		abortTypedError(c, http.StatusForbidden, "approval_scope_not_permitted", "Only a Space manager can do this",
			map[string]any{"required_roles": []string{spaces.RoleManager, spaces.RoleOwner}})
		return nil, false
	}
	return membership, true
}

func (s *Server) listApprovalGrants(c *gin.Context) {
	if !s.approvalRepoAvailable(c) {
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	membership, ok := s.elevatedMembership(c, spaceRef)
	if !ok {
		return
	}
	rules, err := s.spaceRepo.ListApprovalRules(c.Request.Context(), spaceRef, membership.OrgID, c.Query("include_revoked") == "true")
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"grants": rules}})
}

// revokeApprovalGrant revokes a rule. The creator or a current manager/owner may.
// Idempotent.
func (s *Server) revokeApprovalGrant(c *gin.Context) {
	if !s.approvalRepoAvailable(c) {
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	ruleID := strings.TrimSpace(c.Param("rule_id"))
	userID := c.GetString("user_id")
	membership, err := s.spaceRepo.ResolveReadableUserMembership(c.Request.Context(), spaceRef, c.GetString("org_id"), userID)
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	if !spaces.IsElevatedRole(membership.Role) {
		existing, err := s.spaceRepo.GetApprovalRule(c.Request.Context(), spaceRef, membership.OrgID, ruleID)
		if err != nil {
			writeApprovalError(c, err)
			return
		}
		if existing.GrantedBySubjectID != userID {
			abortTypedError(c, http.StatusForbidden, "approval_scope_not_permitted", "Only the creator or a Space manager can revoke this rule", nil)
			return
		}
	}
	rule, err := s.spaceRepo.RevokeApprovalRule(c.Request.Context(), spaceRef, membership.OrgID, ruleID, userID, "revoked", time.Now().UTC())
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"grant": rule}})
}

func (s *Server) getApprovalPolicy(c *gin.Context) {
	if !s.approvalRepoAvailable(c) {
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	membership, err := s.spaceRepo.ResolveReadableUserMembership(c.Request.Context(), spaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	policy, err := s.spaceRepo.GetApprovalPolicy(c.Request.Context(), spaceRef, membership.OrgID)
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"policy": policy}})
}

// putApprovalPolicy sets the Space ceiling, default addressee and quarantine
// mode with a revision compare-and-set. A manager may always tighten; raising
// the ceiling also needs the entitlement and a non-ZDR Space.
func (s *Server) putApprovalPolicy(c *gin.Context) {
	if !s.approvalRepoAvailable(c) {
		return
	}
	var update spaces.ApprovalPolicyUpdate
	if !decodeApprovalBody(c, &update) {
		return
	}
	if err := update.Validate(); err != nil {
		abortTypedError(c, http.StatusBadRequest, "invalid_request", err.Error(), nil)
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	membership, ok := s.elevatedMembership(c, spaceRef)
	if !ok {
		return
	}
	if membership.Kind == spaces.KindPersonal {
		abortTypedError(c, http.StatusForbidden, "space_kind_unsupported", "A personal Space has no approval policy", nil)
		return
	}
	current, err := s.spaceRepo.GetApprovalPolicy(c.Request.Context(), spaceRef, membership.OrgID)
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	if update.Loosens(current) {
		evidence, err := s.spaceRepo.ResolveSharedApprovalDecisionEvidence(c.Request.Context(), spaceRef, membership.OrgID, c.GetString("user_id"))
		if err == nil {
			err = evidence.ValidateForApprovalRuleManage()
		}
		if err != nil {
			writeApprovalError(c, err)
			return
		}
	}
	policy, err := s.spaceRepo.UpsertApprovalPolicy(c.Request.Context(), spaceRef, membership.OrgID, c.GetString("user_id"), update)
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"policy": policy}})
}
