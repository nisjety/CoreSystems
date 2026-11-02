package http

import (
	"errors"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S2: the execution-core surface of Control-held approval authority. The
// caller names a run; Control takes the requester, Space and organisation from
// Session Core's content-free run binding and never from the body.

// toolGateViewLifetime bounds how long Execution Core may cache a gate view
// (the signed decisions it sits beside live 120 s).
const toolGateViewLifetime = 100 * time.Second

type approvalRunRequest struct {
	RunID          string `json:"run_id"`
	OrgID          string `json:"org_id"`
	AgentSubjectID string `json:"agent_subject_id"`
	ToolKey        string `json:"tool_key"`
	ActionKey      string `json:"action_key"`
	SchemaHash     string `json:"schema_hash"`
	ApprovalID     string `json:"approval_id"`
}

// resolveApprovalRun resolves the run binding, answering the typed refusal when
// it is unavailable or the run is no longer active.
func (s *Server) resolveApprovalRun(c *gin.Context, runID, orgID string) (spaces.RunActionAuthority, bool) {
	if s.spaceRepo == nil || s.runActionAuthority == nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval authority is unavailable", nil)
		return spaces.RunActionAuthority{}, false
	}
	if strings.TrimSpace(runID) == "" || strings.TrimSpace(orgID) == "" {
		abortTypedError(c, http.StatusBadRequest, "invalid_request", "run_id and org_id are required", nil)
		return spaces.RunActionAuthority{}, false
	}
	authority, err := s.runActionAuthority.ResolveRunActionAuthority(c.Request.Context(), strings.TrimSpace(runID), strings.TrimSpace(orgID))
	if err != nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval authority is unavailable", nil)
		return spaces.RunActionAuthority{}, false
	}
	if err := authority.Validate(); err != nil {
		abortTypedError(c, http.StatusForbidden, "authority_changed", "The run is no longer active", nil)
		return spaces.RunActionAuthority{}, false
	}
	return authority, true
}

// issueToolGateView answers what Execution Core may fold into its gate for one
// run: the Space ceiling and whether standing rules can apply to this requester
// right now. It carries no content and is allowed under ZDR (grants are simply
// reported disabled). Per-tool levels join in WS10-S5a; `tools` is empty until
// then, which means "no Control-side level", never "allow".
func (s *Server) issueToolGateView(c *gin.Context) {
	var request approvalRunRequest
	if !decodeApprovalBody(c, &request) {
		return
	}
	authority, ok := s.resolveApprovalRun(c, request.RunID, request.OrgID)
	if !ok {
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedApprovalDecisionEvidence(c.Request.Context(), authority.SpaceRef, authority.OrgID, authority.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		abortTypedError(c, http.StatusForbidden, "authority_changed", "Current Space authority is required", nil)
		return
	}
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	policy, err := s.spaceRepo.GetApprovalPolicy(c.Request.Context(), authority.SpaceRef, authority.OrgID)
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	disabledReason := ""
	maxScope := policy.MaxGrantScope
	if useErr := evidence.ValidateForApprovalRuleUse(); useErr != nil {
		disabledReason = "grants_unavailable"
		if denial, isDenial := spaces.AsDenial(useErr); isDenial {
			disabledReason = string(denial.Code)
		}
		maxScope = string(spaces.ApprovalScopeOnce)
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"run_id":                         authority.RunID,
		"space_ref":                      authority.SpaceRef,
		"policy_revision":                policy.Revision,
		"policy_digest":                  spaces.ApprovalGateViewDigest(policy),
		"tools":                          gin.H{},
		"max_grant_scope":                maxScope,
		"default_addressee_kind":         policy.DefaultAddresseeKind,
		"quarantine_mode":                policy.QuarantineMode,
		"standing_rules_enabled":         disabledReason == "",
		"standing_rules_disabled_reason": disabledReason,
		"expires_at":                     time.Now().UTC().Add(toolGateViewLifetime),
	}})
}

// useApprovalGrant tells the gate that a live standing rule covers this exact
// call and hands back a signed proof that Session Core verifies before it
// records an already-granted approval. It widens no authority: the approved
// action still runs as the requester and the owner plane still authorizes it.
func (s *Server) useApprovalGrant(c *gin.Context) {
	var request approvalRunRequest
	if !decodeApprovalBody(c, &request) {
		return
	}
	use := spaces.ApprovalRuleUseRequest{
		RunID: request.RunID, AgentSubjectID: request.AgentSubjectID, ToolKey: request.ToolKey,
		ActionKey: request.ActionKey, SchemaHash: request.SchemaHash,
	}
	if err := use.Validate(); err != nil {
		abortTypedError(c, http.StatusBadRequest, "invalid_request", err.Error(), nil)
		return
	}
	authority, ok := s.resolveApprovalRun(c, request.RunID, request.OrgID)
	if !ok {
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedApprovalDecisionEvidence(c.Request.Context(), authority.SpaceRef, authority.OrgID, authority.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		abortTypedError(c, http.StatusForbidden, "authority_changed", "Current Space authority is required", nil)
		return
	}
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	if err := evidence.ValidateForApprovalRuleUse(); err != nil {
		if denial, isDenial := spaces.AsDenial(err); isDenial && denial.Code == spaces.DenialEntitlementOff {
			abortTypedError(c, http.StatusForbidden, "grants_not_entitled", "Standing approval rules are not enabled for this organization", denial.Details())
			return
		}
		writeApprovalError(c, err)
		return
	}
	// Mint the signing material before the use is counted, so a signer outage
	// cannot burn a use.
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval signer unavailable", nil)
		return
	}
	tokenRef, err := randomDecisionPart()
	if err != nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval entropy unavailable", nil)
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval entropy unavailable", nil)
		return
	}
	now := time.Now().UTC()
	rule, err := s.spaceRepo.UseApprovalRule(c.Request.Context(), evidence, use, now)
	if errors.Is(err, spaces.ErrNoApplicableApprovalRule) {
		abortTypedError(c, http.StatusNotFound, "no_applicable_grant", "No standing approval rule covers this call", nil)
		return
	}
	if err != nil {
		writeApprovalError(c, err)
		return
	}
	claims, err := spaces.NewApprovalGrantUse(evidence, rule, authority.RunID, tokenRef, nonce, now)
	if err != nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval proof could not be built", nil)
		return
	}
	token, err := spaces.SignApprovalGrantUse(key, claims)
	if err != nil {
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval signing failed", nil)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"grant_id": rule.RuleID, "scope": rule.Scope, "token": token, "expires_at": claims.ExpiresAt,
	}})
}

// checkApprovalContinuation is the freshness fence a worker runs before it
// executes an approved action as the requester. It answers authorized only when
// the requester still has the authority the run started with. approval_id names
// the approval being continued; the check itself is the requester's authority.
func (s *Server) checkApprovalContinuation(c *gin.Context) {
	var request approvalRunRequest
	if !decodeApprovalBody(c, &request) {
		return
	}
	if strings.TrimSpace(request.ApprovalID) == "" || len(strings.TrimSpace(request.ApprovalID)) > 200 {
		abortTypedError(c, http.StatusBadRequest, "invalid_request", "approval_id is required", nil)
		return
	}
	authority, ok := s.resolveApprovalRun(c, request.RunID, request.OrgID)
	if !ok {
		return
	}
	evidence, err := s.spaceRepo.ResolveAgentActionDecisionEvidence(c.Request.Context(), authority.SpaceRef, authority.OrgID, authority.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		abortTypedError(c, http.StatusForbidden, "authority_changed", "Current Space authority is required", nil)
		return
	}
	if err != nil {
		if _, isDenial := spaces.AsDenial(err); isDenial {
			abortTypedError(c, http.StatusForbidden, "authority_changed", "Current Space authority no longer allows this", nil)
			return
		}
		abortTypedError(c, http.StatusServiceUnavailable, "approval_authority_unavailable", "Approval authority is unavailable", nil)
		return
	}
	if err := spaces.ValidateCurrentApprovalContinuation(evidence, authority); err != nil {
		abortTypedError(c, http.StatusForbidden, "authority_changed", "Current Space authority no longer matches this run", nil)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"authorized": true}})
}
