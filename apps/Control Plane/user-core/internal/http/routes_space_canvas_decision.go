package http

import (
	"context"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS19-S2: POST /api/v1/internal/spaces/canvas-decision, registered through the
// P-UC1 seam. It is a gateway-delegated issuer route (scope spaces:issue, see
// scopes_space_canvas_decision.go). One route serves the read and the write
// decisions: the operation picks the evidence resolver. A read resolves an
// archived room, a write only an active one, and the decision validator enforces
// the role floor and canvas_entitled.
//
// The body names no actor, organisation, role, audience, policy, revision,
// decision reference, nonce or content. Only the SHA-256 commitment to the
// content is sent, so a signed decision never carries room text.
func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/canvas-decision", s.requireVerifiedSpaceResolver, s.issueCanvasDecision)
	})
}

// spaceCanvasDeniedMessage is the fixed refusal string the BFF maps to its typed
// canvas refusal. spaceCanvasInvalidCode is the typed 400 for a malformed request.
const (
	spaceCanvasDeniedMessage = "Space canvas is not authorized"
	spaceCanvasInvalidCode   = "canvas_decision_invalid"
)

type spaceCanvasDecisionRequest struct {
	SpaceRef             string `json:"space_ref"`
	Op                   string `json:"op"`
	CanvasID             string `json:"canvas_id"`
	ExpectedVersionID    string `json:"expected_version_id"`
	RestoreFromVersionID string `json:"restore_from_version_id"`
	ContentDigest        string `json:"content_digest"`
	IdempotencyKey       string `json:"idempotency_key"`
}

// issueCanvasDecision answers 200 {data:{decision, token, space_kind}} for an
// authorized read or write. A malformed request (an unknown operation, a field the
// operation may not carry, a malformed digest) is a 400 before any authority is
// read. Every authority refusal is a 403 with the fixed message and the typed
// denial code where one exists. A write to an archived room answers the typed 409
// room_archived. With the Space repository unwired the route answers 503.
func (s *Server) issueCanvasDecision(c *gin.Context) {
	if !s.spaceRepoAvailable(c) {
		return
	}
	var request spaceCanvasDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "canvas decision request is invalid", "code": spaceCanvasInvalidCode})
		return
	}
	shape := spaces.CanvasDecisionRequest{
		DecisionRef: "x", Op: request.Op, CanvasID: request.CanvasID,
		ExpectedVersionID: request.ExpectedVersionID, RestoreFromVersionID: request.RestoreFromVersionID,
		ContentDigest: request.ContentDigest, IdempotencyKey: request.IdempotencyKey, Nonce: "x",
	}
	if strings.TrimSpace(request.SpaceRef) == "" || shape.Validate() != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "canvas decision request is invalid", "code": spaceCanvasInvalidCode})
		return
	}
	write := shape.IsWrite()
	personal := func(ctx context.Context, spaceRef, orgID, subjectID string) (spaces.PersonalThreadDecisionEvidence, error) {
		return s.spaceRepo.ResolvePersonalCanvasDecisionEvidence(ctx, spaceRef, orgID, subjectID, write)
	}
	shared := func(ctx context.Context, spaceRef, orgID, subjectID string) (spaces.PersonalThreadDecisionEvidence, error) {
		return s.spaceRepo.ResolveSharedCanvasDecisionEvidence(ctx, spaceRef, orgID, subjectID, write, request.Op)
	}
	var (
		evidence   spaces.PersonalThreadDecisionEvidence
		membership *spaces.CurrentMembership
		ok         bool
	)
	if write {
		evidence, membership, ok = s.resolveThreadWriteEvidence(c, request.SpaceRef, personal, shared, spaceCanvasDeniedMessage)
	} else {
		evidence, membership, ok = s.resolveReadKindEvidence(c, request.SpaceRef, personal, shared, spaceCanvasDeniedMessage)
	}
	if !ok {
		return
	}
	key, decisionRef, nonce, ok := spaceDecisionMaterial(c)
	if !ok {
		return
	}
	decision, err := spaces.IssueCanvasDecision(evidence, spaces.CanvasDecisionRequest{
		DecisionRef: decisionRef, Op: request.Op, CanvasID: request.CanvasID,
		ExpectedVersionID: request.ExpectedVersionID, RestoreFromVersionID: request.RestoreFromVersionID,
		ContentDigest: request.ContentDigest, IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		writeSpaceIssuerDenial(c, err, spaceCanvasDeniedMessage)
		return
	}
	token, ok := signSpaceDecision(c, key, decision)
	if !ok {
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token, "space_kind": membership.Kind}})
}
