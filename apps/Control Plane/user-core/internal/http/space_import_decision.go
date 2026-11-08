package http

import (
	"errors"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS14-S1: the unified import-decision route. POST
// /api/v1/internal/spaces/import-decision {space_ref, source_type,
// idempotency_key} dispatches on the registered Space kind, like
// issueRetrievalDecision: a personal Space gets exactly the decision
// personal-import-decision issues (that route stays for the connector path),
// a shared Space gets the shared import decision. Both carry the same action
// id, schema and digest function, so no verifier changes.
//
// The shared branch sits behind CONTROL_SPACE_SHARED_IMPORT (strict literal
// true, off by default and in production). The flag is a rollout control,
// never an authorization input: when on, every authority check still runs.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/import-decision", s.requireVerifiedSpaceResolver, s.issueImportDecision)
	})
}

// SetSharedImportDecisions wires CONTROL_SPACE_SHARED_IMPORT (strict literal true).
func (s *Server) SetSharedImportDecisions(enabled bool) { s.sharedImportDecisions = enabled }

func writeSharedImportDisabled(c *gin.Context) {
	c.JSON(http.StatusNotFound, gin.H{"error": "shared Space import is not available", "code": "shared_import_disabled"})
}

// writeSharedImportIssuerDenial is writeSpaceIssuerDenial plus the closed
// source_type_not_allowed code for the one refusal that is not an authority
// denial.
func writeSharedImportIssuerDenial(c *gin.Context, err error, message string) {
	if errors.Is(err, spaces.ErrSharedImportSourceType) {
		c.JSON(http.StatusForbidden, gin.H{"error": message, "code": "source_type_not_allowed"})
		return
	}
	writeSpaceIssuerDenial(c, err, message)
}

func (s *Server) issueImportDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request personalImportDecisionRequest
	if !bindSpaceDecisionBody(c, &request, "space_ref, source_type, and idempotency_key are required",
		&request.SpaceRef, &request.IdempotencyKey, &request.SourceType) {
		return
	}
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"),
	)
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectMembershipOnly, "current Space authority required", http.StatusServiceUnavailable, "Space authority unavailable") {
		return
	}
	if membership.Kind == spaces.KindPersonal {
		s.issuePersonalImportDecisionFor(c, request)
		return
	}
	if !s.sharedImportDecisions {
		writeSharedImportDisabled(c)
		return
	}
	evidence, err := s.spaceRepo.ResolveSharedImportDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"),
	)
	if s.writeSpaceDenial(c, err, request.SpaceRef, spaces.ExpectAny, "current Space authority required", http.StatusServiceUnavailable, "Space import authority unavailable") {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueSharedImportDecision(evidence, spaces.PersonalImportDecisionRequest{
		DecisionRef: decisionRef, IdempotencyKey: request.IdempotencyKey, SourceType: request.SourceType, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSharedImportIssuerDenial(c, err, "Space import is not authorized")
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// issueSharedImportExecutionDecision is the shared-Space branch of
// import-execution-decision: Imports Core asks for a fresh Data-targeted
// decision for one already-authorized job intent. Control re-resolves current
// membership, audience, role, entitlement and ZDR now.
func (s *Server) issueSharedImportExecutionDecision(c *gin.Context, intent spaces.PersonalImportExecutionIntent) {
	evidence, err := s.spaceRepo.ResolveSharedImportDecisionEvidence(
		c.Request.Context(), intent.SpaceRef, intent.OrgID, intent.SubjectID,
	)
	if writeSpaceEvidenceError(c, err, "current Space authority required", http.StatusServiceUnavailable, "Space import authority unavailable") {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueSharedImportExecutionDecision(evidence, intent, decisionRef, nonce, time.Now().UTC())
	if err != nil {
		writeSharedImportIssuerDenial(c, err, "Space import execution is not authorized")
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
