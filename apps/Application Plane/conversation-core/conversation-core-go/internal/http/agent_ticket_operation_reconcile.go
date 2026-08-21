package http

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/gin-gonic/gin"
)

// ReconcileAgentTicketOperation is a read-only owner receipt lookup for an
// ambiguous execution-core submission. It verifies the same short-lived
// Control decision and immutable commitments used by the write route, then
// resolves the actor from that decision. It never calls CreateTicketOperation.
func (h *Handler) ReconcileAgentTicketOperation(c *gin.Context) {
	if h == nil || h.service == nil || h.runActionDecisionVerifier == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("agent_action_unavailable", "Agent ticket actions are not configured."))
		return
	}
	var body agentTicketOperationReconcileBody
	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&body); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	if err := validateAgentTicketReconcileBody(body); err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "Ticket reconciliation request is invalid."))
		return
	}
	decision, err := h.runActionDecisionVerifier.Verify(body.ControlDecisionToken)
	if err != nil || decision.RunID != strings.TrimSpace(body.RunID) ||
		decision.OrgID != strings.TrimSpace(body.OrgID) ||
		decision.ActionID != "tickets.create" ||
		decision.ActionSchemaHash != strings.TrimSpace(body.ActionSchemaHash) ||
		decision.ActionSchemaHash != ticketCreateActionContract().SchemaSHA256 ||
		decision.PayloadDigest != strings.TrimSpace(body.PayloadDigest) ||
		decision.IdempotencyKey != strings.TrimSpace(body.IdempotencyKey) {
		c.JSON(http.StatusForbidden, errorPayload("owner_action_denied", "Control decision does not authorize this ticket reconciliation."))
		return
	}
	receipt, err := h.service.GetTicketOperation(
		c.Request.Context(), decision.OrgID, decision.SubjectID, body.IdempotencyKey,
	)
	if err != nil {
		if errors.Is(err, conversation.ErrNotFound) {
			c.JSON(http.StatusNotFound, errorPayload("ticket_operation_not_found", "Ticket operation is not durably recorded."))
			return
		}
		writeServiceError(c, err)
		return
	}
	if receipt.Status == "unknown" {
		c.JSON(http.StatusConflict, gin.H{
			"error": gin.H{"code": "owner_action_unknown", "message": "The owner operation outcome is unknown; reconcile the provider before retrying."},
			"data":  gin.H{"operation": receipt},
		})
		return
	}
	if receipt.Status == "cancelled" {
		c.JSON(http.StatusConflict, gin.H{
			"error": gin.H{"code": "owner_action_cancelled", "message": "The owner operation was cancelled and must not be retried automatically."},
			"data":  gin.H{"operation": receipt},
		})
		return
	}
	if receipt.Status != "completed" {
		c.JSON(http.StatusAccepted, gin.H{"data": gin.H{"operation": receipt}})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"ticket": receipt.Ticket, "operation": receipt}})
}

func validateAgentTicketReconcileBody(body agentTicketOperationReconcileBody) error {
	for name, value := range map[string]string{
		"run_id": body.RunID, "org_id": body.OrgID, "control_decision_token": body.ControlDecisionToken,
		"action_schema_hash": body.ActionSchemaHash, "payload_digest": body.PayloadDigest,
		"idempotency_key": body.IdempotencyKey,
	} {
		value = strings.TrimSpace(value)
		if value == "" || len(value) > 16_384 {
			return errors.New("invalid " + name)
		}
	}
	if !validRunActionCommitment(body.ActionSchemaHash) || !validRunActionCommitment(body.PayloadDigest) {
		return errors.New("invalid ticket commitments")
	}
	return nil
}
