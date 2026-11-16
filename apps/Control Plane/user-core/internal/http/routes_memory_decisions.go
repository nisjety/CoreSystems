package http

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS17-S7: POST /api/v1/internal/spaces/memory-declassify-decision, registered through the P-UC1 seam.
// It is a gateway-delegated issuer route like memory-decision (scope spaces:issue,
// scopes_memory_decisions.go). The body carries no actor, organization, role, audience, policy,
// revision, decision reference or nonce: all of those are resolved or minted here. It carries
// `content_digest` (the digest of the reviewed text, never the text) for a declassify, and no digest for
// a retract. Declassify is refused unless the org's memory_declassify_entitled bit is on and the Space is
// not under ZDR; retract needs neither (removing is never storing).
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/memory-declassify-decision", s.requireVerifiedSpaceResolver, s.issueMemoryDeclassifyDecision)
	})
}

type memoryDeclassifyDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	Operation      string `json:"operation"`
	MemoryID       string `json:"memory_id"`
	ContentDigest  string `json:"content_digest"`
	IdempotencyKey string `json:"idempotency_key"`
}

// memoryDeclassifyDeniedMessage is the single fixed refusal string the BFF maps to the typed
// `memory_declassify_not_authorized`.
const memoryDeclassifyDeniedMessage = "Space memory declassification is not authorized"

// issueMemoryDeclassifyDecision authorizes one declassify (share a personal memory into the room) or one
// retract (remove the room copy the caller shared) by a current editor, manager or owner. Shape and
// operation are checked before any evidence is read: an unknown operation is a 400, never a signed claim.
// Every refusal answers 403 with the same fixed error string plus the typed denial code where one exists.
func (s *Server) issueMemoryDeclassifyDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request memoryDeclassifyDecisionRequest
	if !bindSpaceDecisionBody(c, &request, "space_ref, operation, memory_id, and idempotency_key are required",
		&request.SpaceRef, &request.Operation, &request.MemoryID, &request.IdempotencyKey) {
		return
	}
	if err := (spaces.MemoryDeclassifyDecisionRequest{
		DecisionRef: "x", Operation: request.Operation, MemoryID: request.MemoryID,
		ContentDigest: request.ContentDigest, IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedMemoryDeclassifyDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, memoryDeclassifyDeniedMessage,
		http.StatusForbidden, memoryDeclassifyDeniedMessage) {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueMemoryDeclassifyDecision(evidence, spaces.MemoryDeclassifyDecisionRequest{
		DecisionRef: decisionRef, Operation: request.Operation, MemoryID: request.MemoryID,
		ContentDigest: request.ContentDigest, IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, memoryDeclassifyDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
