package http

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// F-f10-18: POST /api/v1/internal/spaces/skill-write-decision, registered
// through the P-UC1 seam. It issues the Control-signed model.space.skill.write
// decision capability-core verifies before it creates, changes or deletes a
// Space skill. The body carries the Space, the operation, the skill id (empty
// for a create) and the digest of the body the caller will send. It carries no
// actor, organization, role, audience, policy, revision, decision reference,
// idempotency key or nonce: all of those are resolved or minted here. The role
// is never taken from the client.
//
// It sits behind SPACE_SKILL_WRITE_DECISIONS (strict literal true, off by
// default). The switch is a rollout control, never an authorization input:
// when on, every authority check still runs. Off, the route answers 404 with a
// typed code (the router is built before the switch is read, the memory
// decision's pattern). Enable it only together with capability-core's
// SKILL_SPACE_WRITE_VERIFY.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/skill-write-decision", s.requireSpaceSkillWriteIssuer, s.issueSpaceSkillWriteDecision)
	})
}

// SetSpaceSkillWriteDecisions wires SPACE_SKILL_WRITE_DECISIONS (strict literal true).
func (s *Server) SetSpaceSkillWriteDecisions(enabled bool) { s.spaceSkillWriteDecisions = enabled }

// spaceSkillWriteDeniedMessage is the single fixed refusal string the BFF maps
// to a typed Space skill refusal.
const spaceSkillWriteDeniedMessage = "Space skill write is not authorized"

type spaceSkillWriteDecisionRequest struct {
	SpaceRef      string `json:"space_ref"`
	Operation     string `json:"operation"`
	SkillID       string `json:"skill_id"`
	RequestDigest string `json:"request_digest"`
}

// requireSpaceSkillWriteIssuer admits only the gateway workload carrying a
// verified delegation and the dedicated skill-write scope. It is modelled on
// requireSpaceMembershipActor: its own scope, so a deployment grants it to the
// BFF principal alone and the credential is a second kill switch.
func (s *Server) requireSpaceSkillWriteIssuer(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != verevonGatewayPrincipal ||
		!c.GetBool("delegation_verified") ||
		!hasServiceScope(c, spaceSkillWriteIssueScope) ||
		c.GetString("user_id") == "" ||
		c.GetString("org_id") == "" {
		abortTypedError(c, http.StatusForbidden, "space_skill_write_delegation_required",
			"a signed gateway delegation with the Space skill write scope is required", nil)
		return
	}
	c.Next()
}

// issueSpaceSkillWriteDecision authorizes one create, update or delete of a
// Space skill by a current manager or owner of an active shared Space. Shape
// and operation are checked before any evidence is read: an unknown operation
// or a malformed digest is a 400, never a signed claim. Every authority refusal
// (not a member, revoked, personal or archived Space, below the manager floor)
// answers 403 with the same fixed error string plus the typed denial code where
// one exists. With the switch off the route answers 404.
func (s *Server) issueSpaceSkillWriteDecision(c *gin.Context) {
	if !s.spaceSkillWriteDecisions {
		c.JSON(http.StatusNotFound, gin.H{"error": "Space skill write decisions are not available", "code": "space_skill_write_decisions_disabled"})
		return
	}
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request spaceSkillWriteDecisionRequest
	if !bindSpaceDecisionBody(c, &request, "space_ref, operation and request_digest are required",
		&request.SpaceRef, &request.Operation, &request.RequestDigest) {
		return
	}
	if err := (spaces.SpaceSkillWriteDecisionRequest{
		DecisionRef: "x", Operation: request.Operation, SkillID: request.SkillID,
		RequestDigest: request.RequestDigest, IdempotencyKey: "x", Nonce: "x",
	}).Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if err == nil {
		err = spaces.ValidateMembershipForSpaceSkillWrite(membership)
	}
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, spaceSkillWriteDeniedMessage,
		http.StatusForbidden, spaceSkillWriteDeniedMessage) {
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedSpaceSkillWriteDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, spaceSkillWriteDeniedMessage,
		http.StatusForbidden, spaceSkillWriteDeniedMessage) {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	// The BFF sends no idempotency key: each issuance is its own request, so the
	// freshly minted decision reference is the key (unique per decision).
	decision, err := spaces.IssueSpaceSkillWriteDecision(evidence, spaces.SpaceSkillWriteDecisionRequest{
		DecisionRef: decisionRef, Operation: request.Operation, SkillID: request.SkillID,
		RequestDigest: request.RequestDigest, IdempotencyKey: decisionRef, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, spaceSkillWriteDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
