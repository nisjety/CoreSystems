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

// ownerGrantDecisionRequest is deliberately narrow. The verified V3 gateway
// may select a current personal Space and an Application conversation, but it
// cannot select the organization, subject, role, audience, privacy policy, or
// authority revision carried by the resulting decision.
type ownerGrantDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	ConversationID string `json:"conversation_id"`
	ActionID       string `json:"action_id"`
	Operation      string `json:"operation"`
	GrantID        string `json:"grant_id"`
	IdempotencyKey string `json:"idempotency_key"`
}

// issueOwnerGrantDecision provides Control's current personal-Space facts for
// an Application owner to manage its own ticket-action grant. This endpoint
// does not authorize a conversation: Conversation Core must still check its
// verified owner/admin route and persist/revoke its resource grant itself.
func (s *Server) issueOwnerGrantDecision(c *gin.Context) {
	var request ownerGrantDecisionRequest
	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid owner grant intent is required"})
		return
	}
	for _, value := range []string{request.SpaceRef, request.ConversationID, request.ActionID, request.Operation, request.IdempotencyKey} {
		if strings.TrimSpace(value) == "" || len(strings.TrimSpace(value)) > 200 {
			c.JSON(http.StatusBadRequest, gin.H{"error": "valid owner grant intent is required"})
			return
		}
	}
	if len(strings.TrimSpace(request.GrantID)) > 200 {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid owner grant intent is required"})
		return
	}
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalThreadDecisionEvidence(
		c.Request.Context(), strings.TrimSpace(request.SpaceRef), c.GetString("org_id"), c.GetString("user_id"),
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner grant signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner grant entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner grant entropy unavailable"})
		return
	}
	decision, err := spaces.IssueOwnerGrantDecision(evidence, spaces.OwnerGrantDecisionRequest{
		ConversationID: strings.TrimSpace(request.ConversationID),
		ActionID:       strings.TrimSpace(request.ActionID),
		Operation:      strings.TrimSpace(request.Operation),
		GrantID:        strings.TrimSpace(request.GrantID),
		IdempotencyKey: strings.TrimSpace(request.IdempotencyKey),
		DecisionRef:    decisionRef,
		Nonce:          nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "owner grant management is not authorized"})
		return
	}
	token, err := spaces.SignOwnerGrantDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "owner grant signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}
