package http

import (
	"encoding/json"
	"io"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/gin-gonic/gin"
)

type ownerGrantBody struct {
	ControlDecisionToken string `json:"control_decision_token"`
	IdempotencyKey       string `json:"idempotency_key"`
}

// CreateAgentTicketActionGrant creates the owner-side resource intersection
// after both the verified V3 principal and a fresh Control owner-grant token
// agree on the same org, subject, conversation, action, and operation.
func (h *Handler) CreateAgentTicketActionGrant(c *gin.Context) {
	decision, body, principal, ok := h.verifiedOwnerGrantRequest(c, "create", "")
	if !ok {
		return
	}
	receipt, err := h.service.CreateAgentTicketActionGrant(c.Request.Context(), conversation.CreateAgentTicketActionGrantInput{
		OrgID: decision.OrgID, ConversationID: decision.ConversationID, ActionID: decision.ActionID,
		SpaceRef: decision.SpaceRef, SubjectID: decision.SubjectID,
		RecipientAudienceRef: decision.RecipientAudienceRef, RecipientAudienceHash: decision.RecipientAudienceHash,
		RecipientAudienceRevision: decision.RecipientAudienceRevision, PrivacyPolicyRef: decision.PrivacyPolicyRef,
		AuthorityRevision: decision.AuthorityRevision, CreatedByUserID: principal.UserID,
		IdempotencyKey: body.IdempotencyKey, RequestSHA256: ownerGrantRequestSHA256(decision), ControlDecisionRef: decision.DecisionRef,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": receipt})
}

// RevokeAgentTicketActionGrant revokes the exact path grant under an equally
// fresh Control decision. It never accepts a grant ID from a signed create
// decision and never lets a current token from one personal Space revoke
// another Space's resource permission.
func (h *Handler) RevokeAgentTicketActionGrant(c *gin.Context) {
	grantID := strings.TrimSpace(c.Param("grant_id"))
	decision, body, principal, ok := h.verifiedOwnerGrantRequest(c, "revoke", grantID)
	if !ok {
		return
	}
	receipt, err := h.service.RevokeAgentTicketActionGrant(c.Request.Context(), conversation.RevokeAgentTicketActionGrantInput{
		OrgID: decision.OrgID, ConversationID: decision.ConversationID, GrantID: grantID,
		SpaceRef: decision.SpaceRef, SubjectID: decision.SubjectID, RevokedByUserID: principal.UserID,
		IdempotencyKey: body.IdempotencyKey, RequestSHA256: ownerGrantRequestSHA256(decision), ControlDecisionRef: decision.DecisionRef,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": receipt})
}

func (h *Handler) verifiedOwnerGrantRequest(c *gin.Context, operation, pathGrantID string) (ownerGrantDecision, ownerGrantBody, delegatedPrincipalValue, bool) {
	if h == nil || h.service == nil || h.ownerGrantDecisionVerifier == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("owner_grant_unavailable", "Owner grant management is not configured."))
		return ownerGrantDecision{}, ownerGrantBody{}, delegatedPrincipalValue{}, false
	}
	var body ownerGrantBody
	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&body); err != nil || decoder.Decode(&struct{}{}) != io.EOF ||
		strings.TrimSpace(body.ControlDecisionToken) == "" || len(strings.TrimSpace(body.ControlDecisionToken)) > 16_384 ||
		strings.TrimSpace(body.IdempotencyKey) == "" || len(strings.TrimSpace(body.IdempotencyKey)) > 200 {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Owner grant request is invalid."))
		return ownerGrantDecision{}, ownerGrantBody{}, delegatedPrincipalValue{}, false
	}
	principal, present := delegatedPrincipal(c)
	if !present {
		c.JSON(http.StatusForbidden, errorPayload("forbidden", "Verified owner authority is required."))
		return ownerGrantDecision{}, ownerGrantBody{}, delegatedPrincipalValue{}, false
	}
	decision, err := h.ownerGrantDecisionVerifier.Verify(body.ControlDecisionToken)
	if err != nil || decision.Operation != operation || decision.ConversationID != strings.TrimSpace(c.Param("id")) ||
		decision.GrantID != pathGrantID || decision.IdempotencyKey != strings.TrimSpace(body.IdempotencyKey) ||
		decision.OrgID != principal.OrganizationID || decision.SubjectID != principal.UserID {
		c.JSON(http.StatusForbidden, errorPayload("owner_grant_denied", "Control decision does not authorize this owner grant operation."))
		return ownerGrantDecision{}, ownerGrantBody{}, delegatedPrincipalValue{}, false
	}
	return decision, body, delegatedPrincipalValue{UserID: principal.UserID, OrganizationID: principal.OrganizationID}, true
}

// delegatedPrincipalValue keeps the owner management handler independent from
// transport claims beyond the two attributes it must exactly match to the
// signed Control decision.
type delegatedPrincipalValue struct {
	UserID         string
	OrganizationID string
}
