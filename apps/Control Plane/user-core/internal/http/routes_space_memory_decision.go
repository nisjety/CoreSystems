package http

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// EP-8 S7: POST /api/v1/internal/spaces/memory-decision, registered through the
// P-UC1 seam. It is a gateway-delegated issuer route like run-control-decision
// (scope spaces:issue, scopes_space_memory_decision.go). The body carries no
// actor, organization, role, audience, policy, revision, decision reference,
// idempotency key or nonce: all of those are resolved or minted here.
//
// It sits behind CONTROL_SPACE_MEMORY_DECISIONS (strict literal true, off by
// default and in production). The flag is a rollout control, never an
// authorization input: when on, every authority check still runs.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/memory-decision", s.requireVerifiedSpaceResolver, s.issueSpaceMemoryDecision)
	})
}

// SetSpaceMemoryDecisions wires CONTROL_SPACE_MEMORY_DECISIONS (strict literal true).
func (s *Server) SetSpaceMemoryDecisions(enabled bool) { s.spaceMemoryDecisions = enabled }

// spaceMemoryDeniedMessage is the single fixed refusal string the BFF maps to
// the typed `space_memory_not_authorized`.
const spaceMemoryDeniedMessage = "Space memory management is not authorized"

type spaceMemoryDecisionRequest struct {
	SpaceRef  string `json:"space_ref"`
	Operation string `json:"operation"`
	MemoryID  string `json:"memory_id"`
}

// issueSpaceMemoryDecision authorizes one list, one row delete or one agent
// erase of a shared Space's memory by a current manager or owner. Shape and
// operation are checked before any evidence is read: an unknown operation is a
// 400, never a signed claim. Every authority refusal (not a member, personal
// Space, below the manager floor) answers 403 with the same fixed error string
// plus the typed denial code where one exists. With the switch off the route
// answers 404, which the BFF reports as not authorized.
func (s *Server) issueSpaceMemoryDecision(c *gin.Context) {
	if !s.spaceMemoryDecisions {
		c.JSON(http.StatusNotFound, gin.H{"error": "Space memory management is not available", "code": "space_memory_decisions_disabled"})
		return
	}
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request spaceMemoryDecisionRequest
	if !bindSpaceDecisionBody(c, &request, "space_ref and operation are required",
		&request.SpaceRef, &request.Operation) {
		return
	}
	if err := (spaces.SpaceMemoryDecisionRequest{
		DecisionRef: "x", Operation: request.Operation, MemoryID: request.MemoryID,
		IdempotencyKey: "x", Nonce: "x",
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err == nil {
		err = spaces.ValidateMembershipForSpaceMemoryManage(membership)
	}
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, spaceMemoryDeniedMessage,
		http.StatusForbidden, spaceMemoryDeniedMessage) {
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedSpaceMemoryDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, spaceMemoryDeniedMessage,
		http.StatusForbidden, spaceMemoryDeniedMessage) {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	// The BFF sends no idempotency key: each issuance is its own request, so the
	// freshly minted decision reference is the key (unique per decision).
	decision, err := spaces.IssueSpaceMemoryDecision(evidence, spaces.SpaceMemoryDecisionRequest{
		DecisionRef: decisionRef, Operation: request.Operation, MemoryID: request.MemoryID,
		IdempotencyKey: decisionRef, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, spaceMemoryDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
