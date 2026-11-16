package http

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS17-S8: POST /api/v1/internal/spaces/memory-personal-context-decision, registered through the P-UC1 seam. It is a
// gateway-delegated issuer route like memory-declassify-decision (scope spaces:issue, scopes_memory_personal_context.go).
// The body carries the Space, the thread the turn belongs to and the turn's idempotency key. The actor, organization,
// role, audience, policy, revisions, decision reference and nonce are resolved or minted here, never read from the body.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/memory-personal-context-decision", s.requireVerifiedSpaceResolver, s.issueMemoryPersonalContextDecision)
	})
}

type memoryPersonalContextDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	ThreadID       string `json:"thread_id"`
	IdempotencyKey string `json:"idempotency_key"`
}

// memoryPersonalContextDeniedMessage is the single fixed refusal string the BFF maps to the typed
// `personal_memory_not_authorized`.
const memoryPersonalContextDeniedMessage = "Personal memory use in this Space is not authorized"

// issueMemoryPersonalContextDecision authorizes one turn's personal memory for a current editor, manager or owner of
// a shared Space. Shape is checked before any evidence is read. Every refusal answers 403 with the same fixed message,
// plus the typed denial code where Control gave one.
func (s *Server) issueMemoryPersonalContextDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request memoryPersonalContextDecisionRequest
	if !bindSpaceDecisionBody(c, &request, "space_ref, thread_id, and idempotency_key are required",
		&request.SpaceRef, &request.ThreadID, &request.IdempotencyKey) {
		return
	}
	if err := (spaces.MemoryPersonalContextDecisionRequest{
		DecisionRef: "x", ThreadID: request.ThreadID, IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedMemoryPersonalContextDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, memoryPersonalContextDeniedMessage,
		http.StatusForbidden, memoryPersonalContextDeniedMessage) {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueMemoryPersonalContextDecision(evidence, spaces.MemoryPersonalContextDecisionRequest{
		DecisionRef: decisionRef, ThreadID: request.ThreadID, IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, memoryPersonalContextDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
