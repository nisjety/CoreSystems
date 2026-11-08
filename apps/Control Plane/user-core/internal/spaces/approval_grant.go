package spaces

import (
	"fmt"
	"strings"
	"time"
)

// WS10-S2: a standing approval rule. It waives one prompt-class decision for
// one tool of one agent in one Space; it never widens owner-plane authority
// (the approved action still runs as the requester and is authorized by the
// owner plane at effect time), and a current `block` or ZDR still wins.

// ApprovalRuleReviewWindow is how long a rule stays usable before a manager must
// confirm it again (plan: review_by 90 d). A rule past review_by is not applied.
const ApprovalRuleReviewWindow = 90 * 24 * time.Hour

// ApprovalRule is one stored standing rule.
type ApprovalRule struct {
	RuleID             string     `json:"rule_id"`
	OrgID              string     `json:"org_id"`
	SpaceRef           string     `json:"space_ref"`
	Scope              string     `json:"scope"`
	AgentSubjectID     string     `json:"agent_subject_id"`
	ToolKey            string     `json:"tool_key"`
	ActionKey          string     `json:"action_key"`
	ActionSchemaHash   string     `json:"action_schema_hash"`
	SourceApprovalID   string     `json:"source_approval_id,omitempty"`
	GrantedBySubjectID string     `json:"granted_by_subject_id"`
	GrantedByRole      string     `json:"granted_by_role"`
	AuthorityRevision  int64      `json:"authority_revision"`
	CreatedAt          time.Time  `json:"created_at"`
	ReviewBy           time.Time  `json:"review_by"`
	RevokedAt          *time.Time `json:"revoked_at,omitempty"`
	RevokedBy          string     `json:"revoked_by,omitempty"`
	RevokeReason       string     `json:"revoke_reason,omitempty"`
	UseCount           int64      `json:"use_count"`
	LastUsedAt         *time.Time `json:"last_used_at,omitempty"`
}

// Live reports whether the rule can pass a prompt at now.
func (r ApprovalRule) Live(now time.Time) bool {
	return r.RevokedAt == nil && now.Before(r.ReviewBy)
}

// ApprovalRuleRequest is the body of a rule creation. It names what the rule
// covers; the actor, organisation, role and revisions are resolved by Control.
type ApprovalRuleRequest struct {
	SpaceRef         string
	AgentSubjectID   string
	ToolKey          string
	ActionKey        string
	ActionSchemaHash string
	SourceApprovalID string
	IdempotencyKey   string
}

// Validate checks shape and eligibility. It refuses every tool a standing rule
// may never cover, so a caller cannot obtain a rule for a provider write by
// asking for it.
func (r ApprovalRuleRequest) Validate() error {
	for name, value := range map[string]string{
		"space_ref": r.SpaceRef, "agent_subject_id": r.AgentSubjectID, "tool_key": r.ToolKey,
		"action_key": r.ActionKey, "action_schema_hash": r.ActionSchemaHash,
		"idempotency_key": r.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("approval rule %s is required", name)
		}
	}
	if len(strings.TrimSpace(r.SourceApprovalID)) > maxApprovalIDLen ||
		len(strings.TrimSpace(r.AgentSubjectID)) > maxApprovalIDLen ||
		len(strings.TrimSpace(r.SpaceRef)) > maxApprovalIDLen {
		return fmt.Errorf("approval rule identifier is oversized")
	}
	if n := len(strings.TrimSpace(r.IdempotencyKey)); n < approvalIdempotencyLo || n > approvalIdempotencyHi {
		return fmt.Errorf("approval rule idempotency_key must be %d to %d characters", approvalIdempotencyLo, approvalIdempotencyHi)
	}
	if !validSHA256Commitment(r.ActionSchemaHash) {
		return fmt.Errorf("approval rule action_schema_hash is invalid")
	}
	if !StandingRuleEligibleAction(r.ToolKey, r.ActionKey) {
		return ErrApprovalScopeNotPermitted
	}
	return nil
}

// ApprovalRuleUseRequest names the call a gate wants to pass. Execution Core
// presents it; the requester, Space and organisation come from Session Core's
// run binding, never from the body.
type ApprovalRuleUseRequest struct {
	RunID          string
	AgentSubjectID string
	ToolKey        string
	ActionKey      string
	SchemaHash     string
}

// Validate checks shape only.
func (r ApprovalRuleUseRequest) Validate() error {
	for name, value := range map[string]string{
		"run_id": r.RunID, "agent_subject_id": r.AgentSubjectID, "tool_key": r.ToolKey,
		"action_key": r.ActionKey, "schema_hash": r.SchemaHash,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("approval rule use %s is required", name)
		}
	}
	if len(strings.TrimSpace(r.RunID)) > maxApprovalIDLen || len(strings.TrimSpace(r.AgentSubjectID)) > maxApprovalIDLen {
		return fmt.Errorf("approval rule use identifier is oversized")
	}
	if !validSHA256Commitment(r.SchemaHash) {
		return fmt.Errorf("approval rule use schema_hash is invalid")
	}
	return nil
}

// ValidateCurrentApprovalContinuation is the effect-time freshness fence for an
// approved action that a worker is about to run as the requester. It compares
// the run's immutable binding with Control's freshly resolved authority for the
// same requester, exactly as the run-action fence does: a removed member, a
// changed audience, privacy or authority revision all make the continuation
// ineligible. The worker acknowledges that as `authority_changed`.
func ValidateCurrentApprovalContinuation(evidence PersonalThreadDecisionEvidence, authority RunActionAuthority) error {
	if err := evidence.ValidateForAgentAction(); err != nil {
		return fmt.Errorf("current agent action authority is ineligible: %w", err)
	}
	expectedSourceRef := fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	if !runClaimsMatchEvidence(
		evidence, expectedSourceRef,
		authority.OrgID, authority.SpaceRef, authority.SubjectID,
		authority.RecipientAudienceRef, authority.RecipientAudienceHash, authority.PrivacyPolicyRef,
		authority.RunContextAuthorizationRef,
		authority.RecipientAudienceRevision, authority.AuthorityRevision,
	) {
		return fmt.Errorf("approval continuation does not match current Control authority")
	}
	return nil
}
