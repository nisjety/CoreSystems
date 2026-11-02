package http

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS09-S7: POST /api/v1/internal/spaces/run-control-decision, registered through
// the P-UC1 seam. It is a gateway-delegated issuer route like thread-read-decision
// (scope spaces:issue, scopes_run_control.go) and is unused until the manager
// Stop path (WS09-S8) calls it. The body carries no actor, organization, role,
// audience, policy, revision, decision reference or nonce: all of those are
// resolved or minted here.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/run-control-decision", s.requireVerifiedSpaceResolver, s.issueRunControlDecision)
	})
}

type runControlDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	RunID          string `json:"run_id"`
	Operation      string `json:"operation"`
	IdempotencyKey string `json:"idempotency_key"`
}

// runControlDeniedMessage is the single fixed refusal string the BFF maps to the
// typed `space_run_control_not_authorized`.
const runControlDeniedMessage = "Space run control is not authorized"

// issueRunControlDecision authorizes one cancel of one run in a shared Space by
// a current manager or owner whose org has run_control_entitled. Shape and
// operation are checked before any evidence is read: an unknown operation is a
// 400, never a signed claim. Every refusal (not a member, below the manager
// floor, entitlement off, personal Space) answers 403 with the same fixed error
// string plus the typed denial code where one exists.
func (s *Server) issueRunControlDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request runControlDecisionRequest
	if !bindSpaceDecisionBody(c, &request, "space_ref, run_id, operation, and idempotency_key are required",
		&request.SpaceRef, &request.RunID, &request.Operation, &request.IdempotencyKey) {
		return
	}
	if err := (spaces.RunControlDecisionRequest{
		DecisionRef: "x", RunID: request.RunID, Operation: request.Operation,
		IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedRunControlDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, runControlDeniedMessage,
		http.StatusForbidden, runControlDeniedMessage) {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueSharedRunControlDecision(evidence, spaces.RunControlDecisionRequest{
		DecisionRef: decisionRef, RunID: request.RunID, Operation: request.Operation,
		IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, runControlDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
