package http

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// runWakeDecisionRequest is the body of the wake issuer. Like the append
// issuer it carries no actor, organization, role, audience, policy, revision,
// decision reference or nonce: all of those are resolved or minted here. The
// content is never sent, only its SHA-256 commitment.
type runWakeDecisionRequest struct {
	SpaceRef         string `json:"space_ref"`
	ThreadID         string `json:"thread_id"`
	TriggerMessageID string `json:"trigger_message_id"`
	ContentDigest    string `json:"content_digest"`
	AgentSubjectID   string `json:"agent_subject_id"`
	TurnOrigin       string `json:"turn_origin"`
	IdempotencyKey   string `json:"idempotency_key"`
}

// issueRunWakeDecision authorizes one run start from one stored human post for
// one bound agent. Evidence is resolved fresh (the same personal/shared write
// evidence a thread append uses, so editor and above only), the agent must be
// an active service member of this Space, and only turn_origin "direct" is
// issued. A refusal answers the single fixed string the BFF maps to the typed
// `wake_not_authorized`; a malformed request is a 400.
func (s *Server) issueRunWakeDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request runWakeDecisionRequest
	if !bindSpaceDecisionBody(c, &request,
		"space_ref, thread_id, trigger_message_id, content_digest, agent_subject_id, turn_origin, and idempotency_key are required",
		&request.SpaceRef, &request.ThreadID, &request.TriggerMessageID, &request.ContentDigest,
		&request.AgentSubjectID, &request.TurnOrigin, &request.IdempotencyKey) {
		return
	}
	// Shape and origin are checked before any evidence is read: an unknown or
	// non-direct origin is a 400, never a signed claim.
	if err := (spaces.RunWakeDecisionRequest{
		DecisionRef: "x", ThreadID: request.ThreadID, TriggerMessageID: request.TriggerMessageID,
		ContentDigest: request.ContentDigest, AgentSubjectID: request.AgentSubjectID,
		TurnOrigin: request.TurnOrigin, IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	evidence, _, ok := s.resolveThreadWriteEvidence(c, request.SpaceRef,
		s.spaceRepo.ResolvePersonalThreadDecisionEvidence, s.spaceRepo.ResolveSharedThreadDecisionEvidence,
		"current recipient audience authority required")
	if !ok {
		return
	}
	roster, err := s.spaceRepo.RosterForSpace(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space roster resolution failed"})
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueRunWakeDecision(evidence, roster, spaces.RunWakeDecisionRequest{
		DecisionRef: decisionRef, ThreadID: request.ThreadID, TriggerMessageID: request.TriggerMessageID,
		ContentDigest: request.ContentDigest, AgentSubjectID: request.AgentSubjectID,
		TurnOrigin: request.TurnOrigin, IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, "Space run wake is not authorized")
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
