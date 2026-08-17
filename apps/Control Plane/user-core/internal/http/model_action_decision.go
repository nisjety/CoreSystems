package http

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

// runActionDecisionRequest contains only immutable identifiers and commitments
// from the execution lane. In particular, it has no subject, Space, audience,
// privacy, role, target resource, or authority revision: Control resolves all
// of those independently and target resources remain owner-authorized.
type runActionDecisionRequest struct {
	RunID            string `json:"run_id"`
	OrgID            string `json:"org_id"`
	ActionID         string `json:"action_id"`
	ActionSchemaHash string `json:"action_schema_hash"`
	PayloadDigest    string `json:"payload_digest"`
	IdempotencyKey   string `json:"idempotency_key"`
}

// modelActionViewRequest intentionally has no action, subject, Space, target
// resource, or policy fields. Control derives the only viewable action from
// independently resolved current authority; clients can name only the source
// run whose eligibility they need rechecked.
type modelActionViewRequest struct {
	RunID string `json:"run_id"`
	OrgID string `json:"org_id"`
}

// issueModelActionView issues a short-lived, view-only token for Capability
// Core. It is not an owner-action decision: the Model still needs a fresh
// payload-bound decision and the owning Conversation Core authorization before
// any effect. Returning only the bearer keeps resolved authority claims local
// to service-to-service consumers and out of browser/API contracts.
func (s *Server) issueModelActionView(c *gin.Context) {
	var request modelActionViewRequest
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, 16*1024))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil || decoder.Decode(&struct{}{}) != io.EOF || strings.TrimSpace(request.RunID) == "" || strings.TrimSpace(request.OrgID) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid model action run is required"})
		return
	}
	if s.spaceRepo == nil || s.runActionAuthority == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model action authority is unavailable"})
		return
	}
	authority, err := s.runActionAuthority.ResolveRunActionAuthority(c.Request.Context(), strings.TrimSpace(request.RunID), strings.TrimSpace(request.OrgID))
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model action authority is unavailable"})
		return
	}
	evidence, err := s.spaceRepo.ResolveAgentActionDecisionEvidence(c.Request.Context(), authority.SpaceRef, authority.OrgID, authority.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "current Space authority is unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model action signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model action entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model action entropy unavailable"})
		return
	}
	view, err := spaces.IssueModelActionView(evidence, authority, spaces.ModelActionViewRequest{
		DecisionRef: decisionRef,
		Nonce:       nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "model action is not authorized"})
		return
	}
	token, err := spaces.SignModelActionView(key, view)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "model action signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"token": token}})
}

// issueRunActionDecision emits one short-lived, owner-targeted decision. It
// fails closed when either Control's current Space evidence or Session Core's
// immutable run binding is absent or inconsistent. No run content travels
// through this endpoint.
func (s *Server) issueRunActionDecision(c *gin.Context) {
	if s.spaceRepo == nil || s.runActionAuthority == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "run action authority is unavailable"})
		return
	}
	var request runActionDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid run action intent is required"})
		return
	}
	for _, value := range []string{request.RunID, request.OrgID, request.ActionID, request.ActionSchemaHash, request.PayloadDigest, request.IdempotencyKey} {
		if strings.TrimSpace(value) == "" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "valid run action intent is required"})
			return
		}
	}

	// org_id is an opaque lookup selector only. Session Core authenticates
	// Control and returns its own bound org/subject/Space; a forged selector
	// cannot create an authority assertion.
	authority, err := s.runActionAuthority.ResolveRunActionAuthority(c.Request.Context(), strings.TrimSpace(request.RunID), strings.TrimSpace(request.OrgID))
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "run action authority is unavailable"})
		return
	}
	evidence, err := s.spaceRepo.ResolveAgentActionDecisionEvidence(c.Request.Context(), authority.SpaceRef, authority.OrgID, authority.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "current Space authority is unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "run action signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "run action entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "run action entropy unavailable"})
		return
	}
	decision, err := spaces.IssueRunActionDecision(evidence, authority, spaces.RunActionDecisionRequest{
		ActionID: strings.TrimSpace(request.ActionID), ActionSchemaHash: strings.TrimSpace(request.ActionSchemaHash),
		PayloadDigest: strings.TrimSpace(request.PayloadDigest), IdempotencyKey: strings.TrimSpace(request.IdempotencyKey),
		DecisionRef: decisionRef, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "run action is not authorized"})
		return
	}
	token, err := spaces.SignRunActionDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "run action signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// checkCurrentRunActionAuthority is the owner-plane freshness fence for a
// previously signed run-action decision. The caller presents non-secret
// claims only; Conversation Core has already verified the bearer and still
// performs its independent conversation grant/transaction check after this
// returns. Any Control lookup or mismatch fails closed.
func (s *Server) checkCurrentRunActionAuthority(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "run action authority is unavailable"})
		return
	}
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, 16*1024))
	decoder.DisallowUnknownFields()
	var decision spaces.RunActionDecision
	if err := decoder.Decode(&decision); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid run action authority is required"})
		return
	}
	if err := decision.Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid run action authority is required"})
		return
	}
	evidence, err := s.spaceRepo.ResolveAgentActionDecisionEvidence(c.Request.Context(), decision.SpaceRef, decision.OrgID, decision.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "current Space authority is unavailable"})
		return
	}
	if err := spaces.ValidateCurrentRunActionDecision(evidence, decision); err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Control authority does not authorize this action"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"authorized": true}})
}
