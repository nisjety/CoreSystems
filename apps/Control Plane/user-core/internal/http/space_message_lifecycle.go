package http

import (
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// threadReviseDecisionRequest is the body of the revise issuer. Like the append
// issuer it carries no actor, organization, role, audience, policy, retention,
// revision, decision reference or nonce: all of those are resolved or minted
// here. The content is never sent, only its SHA-256 commitment (the commitment
// of the empty string for a delete).
type threadReviseDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	ThreadID       string `json:"thread_id"`
	MessageID      string `json:"message_id"`
	Action         string `json:"action"`
	ContentDigest  string `json:"content_digest"`
	IdempotencyKey string `json:"idempotency_key"`
}

// issueThreadReviseDecision authorizes revising one stored message. Evidence is
// resolved fresh (active Space, current recipient audience, editor floor and
// above, message_revise_entitled, not Zero Data Retention). An unknown action or
// a malformed digest is a 400 before any evidence is read; a refusal answers the
// fixed string the BFF maps to the typed `space_message_revise_not_authorized`.
func (s *Server) issueThreadReviseDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request threadReviseDecisionRequest
	if !bindSpaceDecisionBody(c, &request,
		"space_ref, thread_id, message_id, action, content_digest, and idempotency_key are required",
		&request.SpaceRef, &request.ThreadID, &request.MessageID, &request.Action,
		&request.ContentDigest, &request.IdempotencyKey) {
		return
	}
	if err := (spaces.ThreadReviseDecisionRequest{
		DecisionRef: "x", ThreadID: request.ThreadID, MessageID: request.MessageID,
		Action: request.Action, ContentDigest: request.ContentDigest,
		IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	evidence, _, ok := s.resolveThreadWriteEvidence(c, request.SpaceRef,
		s.spaceRepo.ResolvePersonalThreadReviseDecisionEvidence, s.spaceRepo.ResolveSharedThreadReviseDecisionEvidence,
		"current recipient audience authority required")
	if !ok {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueThreadReviseDecision(evidence, spaces.ThreadReviseDecisionRequest{
		DecisionRef: decisionRef, ThreadID: request.ThreadID, MessageID: request.MessageID,
		Action: request.Action, ContentDigest: request.ContentDigest,
		IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, "Space message revision is not authorized")
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// getSpaceMessagePolicy returns the lifecycle hints for the delegated member. It
// never authorizes anything: the decision route still re-resolves every fact.
func (s *Server) getSpaceMessagePolicy(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	hints, err := s.spaceRepo.MessagePolicyHints(c.Request.Context(), spaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, spaceRef, spaces.ExpectMembershipOnly, "current Space authority required", http.StatusServiceUnavailable, "Space authority unavailable") {
		return
	}
	c.Header("Cache-Control", "no-store")
	c.JSON(http.StatusOK, gin.H{"data": hints})
}

type messagePolicyBody struct {
	OrgID             string `json:"org_id"`
	OriginalRetention string `json:"original_retention"`
}

// upsertSpaceMessagePolicy is the operator path that sets an organization's
// original-text rule. Control's own policy principal only; one evidence row is
// written per attempt.
func (s *Server) upsertSpaceMessagePolicy(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var body messagePolicyBody
	if !decodeStrictBody(c, &body, false) {
		return
	}
	policy := spaces.MessagePolicy{OrgID: strings.TrimSpace(body.OrgID), OriginalRetention: strings.TrimSpace(body.OriginalRetention)}
	if policy.OrgID == "" {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "org_id is required",
			map[string]any{"field": "org_id"})
		return
	}
	if !spaces.ValidOriginalRetention(policy.OriginalRetention) {
		abortTypedError(c, http.StatusBadRequest, spaces.CodeInvalidRequest, "original_retention must be retain or purge",
			map[string]any{"field": "original_retention"})
		return
	}
	if err := s.spaceRepo.UpsertMessagePolicy(c.Request.Context(), policy, c.GetString("service_id")); err != nil {
		abortTypedError(c, http.StatusServiceUnavailable, "space_policy_unavailable", "the message policy could not be saved", nil)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"org_id": policy.OrgID, "original_retention": policy.OriginalRetention}})
}
