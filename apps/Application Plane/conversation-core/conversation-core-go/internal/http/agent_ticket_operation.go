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

var ErrRunActionAuthorityDenied = errors.New("current Control run action authority denied")

// CreateAgentTicketOperation is the private owner-plane effect boundary for a
// Model-proposed ticket. Transport authentication proves only execution-core;
// the signed Control decision supplies the current subject and organization.
// It carries a source-run context reference as provenance, never as a ticket
// or conversation permission. Conversation Core always resolves the target
// conversation in the decision's organization immediately before its durable
// operation write.
func (h *Handler) CreateAgentTicketOperation(c *gin.Context) {
	if h == nil || h.service == nil || h.runActionDecisionVerifier == nil ||
		h.runActionAuthorityValidator == nil || h.ownerEffectReservationCoordinator == nil ||
		!h.ownerEffectReservationCoordinator.Ready() {
		c.JSON(http.StatusServiceUnavailable, errorPayload("agent_action_unavailable", "Agent ticket actions are not configured."))
		return
	}
	var body agentTicketCreateBody
	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&body); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	if err := validateAgentTicketCreateBody(body); err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "Agent ticket request is invalid."))
		return
	}
	decision, err := h.runActionDecisionVerifier.Verify(body.ControlDecisionToken)
	if err != nil || decision.RunID != strings.TrimSpace(body.RunID) ||
		decision.ActionSchemaHash != ticketCreateActionContract().SchemaSHA256 ||
		decision.IdempotencyKey != strings.TrimSpace(body.IdempotencyKey) ||
		decision.PayloadDigest != agentTicketPayloadDigest(decision.RunID, decision.OrgID, body) ||
		(strings.TrimSpace(body.OwnerUserID) != "" && decision.SubjectID != strings.TrimSpace(body.OwnerUserID)) {
		c.JSON(http.StatusForbidden, errorPayload("owner_action_denied", "Control decision does not authorize this ticket operation."))
		return
	}
	// Signature verification proves who issued the decision, but not that its
	// membership, audience, privacy, entitlement, or ZDR facts are still
	// current. Re-resolve those facts before creating a Control reservation.
	if err := h.runActionAuthorityValidator.ValidateRunActionAuthority(c.Request.Context(), decision); err != nil {
		writeRunActionAuthorityError(c, err)
		return
	}
	// Reserve and commit are the Control-owned linearization point. A prior
	// "current authority" read cannot close a revocation race because Control
	// could change after that read but before this owner transaction commits.
	// The signed bearer travels only to Control for these two direct hops.
	authorization := conversation.AgentTicketActionAuthorization{
		DecisionRef:               decision.DecisionRef,
		SpaceRef:                  decision.SpaceRef,
		SubjectID:                 decision.SubjectID,
		RecipientAudienceRef:      decision.RecipientAudienceRef,
		RecipientAudienceHash:     decision.RecipientAudienceHash,
		RecipientAudienceRevision: decision.RecipientAudienceRevision,
		PrivacyPolicyRef:          decision.PrivacyPolicyRef,
		AuthorityRevision:         decision.AuthorityRevision,
	}
	createInput := conversation.CreateTicketInput{
		OrgID:                    decision.OrgID,
		ConversationID:           strings.TrimSpace(body.ConversationID),
		WorkType:                 strings.ToLower(strings.TrimSpace(body.WorkType)),
		Priority:                 strings.ToLower(strings.TrimSpace(body.Priority)),
		Severity:                 strings.ToLower(strings.TrimSpace(body.Severity)),
		Category:                 strings.TrimSpace(body.Category),
		Intent:                   strings.TrimSpace(body.Intent),
		Source:                   "approved_ai_action",
		CreatedBy:                decision.SubjectID,
		ActorUserID:              decision.SubjectID,
		IdempotencyKey:           strings.TrimSpace(body.IdempotencyKey),
		AgentActionAuthorization: &authorization,
	}
	grantRef, err := h.service.ResolveAgentTicketActionGrant(c.Request.Context(), createInput)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	authorization.GrantRef = grantRef
	commitment := ownerEffectReservationCommitment{
		OperationID:      conversation.TicketOperationID(decision.OrgID, body.IdempotencyKey),
		ActionID:         decision.ActionID,
		ActionSchemaHash: decision.ActionSchemaHash,
		PayloadDigest:    decision.PayloadDigest,
		IdempotencyKey:   decision.IdempotencyKey,
		DecisionRef:      decision.DecisionRef,
		// The grant ID is opaque, but it is an immutable owner-side consent
		// record. The final ticket transaction locks and rechecks this exact
		// ID; a revoke/regrant cannot inherit a previously committed receipt.
		GrantRef: grantRef,
	}
	createInput.ActionID = decision.ActionID
	createInput.OperationID = commitment.OperationID
	createInput.RequestSHA256 = conversation.TicketOperationRequestSHA256(createInput)
	intent, err := h.service.BeginAgentTicketOperationIntent(c.Request.Context(), conversation.TicketOperationIntentInput{
		OperationID:      commitment.OperationID,
		OrgID:            decision.OrgID,
		IdempotencyKey:   commitment.IdempotencyKey,
		ActionID:         commitment.ActionID,
		ActorUserID:      decision.SubjectID,
		ConversationID:   createInput.ConversationID,
		RequestSHA256:    createInput.RequestSHA256,
		ActionSchemaHash: commitment.ActionSchemaHash,
		PayloadDigest:    commitment.PayloadDigest,
		DecisionRef:      commitment.DecisionRef,
		GrantRef:         commitment.GrantRef,
	})
	if err != nil {
		if errors.Is(err, conversation.ErrTicketOperationIntentUnavailable) {
			c.JSON(http.StatusServiceUnavailable, errorPayload("agent_action_unavailable", "Ticket owner intent storage is not configured."))
		} else {
			writeServiceError(c, err)
		}
		return
	}
	if intent == nil || intent.OperationID != commitment.OperationID || intent.Status == "" {
		c.JSON(http.StatusServiceUnavailable, errorPayload("agent_action_unavailable", "Ticket owner intent storage returned an incomplete receipt."))
		return
	}
	if intent.Status == "completed" {
		c.JSON(http.StatusOK, gin.H{"data": gin.H{"ticket": intent.Ticket, "operation": intent}})
		return
	}
	if intent.Status == "unknown" || intent.Status == "cancelled" {
		c.JSON(http.StatusConflict, gin.H{
			"error": gin.H{"code": "owner_action_unknown", "message": "The ticket operation has a durable unresolved outcome; reconcile before retrying."},
			"data":  gin.H{"operation": intent},
		})
		return
	}
	reservation := ownerEffectReservationReceipt{Status: intent.Status, ReservationID: intent.ControlReservationID, OperationID: commitment.OperationID}
	if reservation.Status == "pending_control_commit" {
		reservation, err = h.ownerEffectReservationCoordinator.Reserve(c.Request.Context(), body.ControlDecisionToken, commitment)
		if err != nil {
			writeOwnerEffectReservationCoordinatorError(c, err)
			return
		}
	}
	if reservation.Status != "reserved" && reservation.Status != "committed" {
		c.JSON(http.StatusForbidden, errorPayload("owner_action_denied", "Control did not reserve this ticket operation."))
		return
	}
	if reservation.Status != "committed" {
		reservation, err = h.ownerEffectReservationCoordinator.Commit(c.Request.Context(), reservation.ReservationID, body.ControlDecisionToken, commitment)
		if err != nil {
			writeOwnerEffectReservationCoordinatorError(c, err)
			return
		}
	}
	if reservation.Status != "committed" || reservation.ReservationID == "" || reservation.OperationID != commitment.OperationID {
		c.JSON(http.StatusServiceUnavailable, errorPayload("agent_action_unavailable", "Control returned an incomplete owner-effect receipt."))
		return
	}
	bound, err := h.service.BindAgentTicketOperationReservation(c.Request.Context(), conversation.TicketOperationReservationInput{
		TicketOperationIntentInput: conversation.TicketOperationIntentInput{
			OperationID: commitment.OperationID, OrgID: decision.OrgID, IdempotencyKey: commitment.IdempotencyKey,
			ActionID: commitment.ActionID, ActorUserID: decision.SubjectID, ConversationID: createInput.ConversationID,
			RequestSHA256: createInput.RequestSHA256, ActionSchemaHash: commitment.ActionSchemaHash,
			PayloadDigest: commitment.PayloadDigest, DecisionRef: commitment.DecisionRef, GrantRef: commitment.GrantRef,
		},
		ControlReservationID: reservation.ReservationID,
	})
	if err != nil {
		_ = h.service.MarkAgentTicketOperationUnknown(c.Request.Context(), conversation.TicketOperationOutcomeInput{
			OrgID: decision.OrgID, OperationID: commitment.OperationID, IdempotencyKey: commitment.IdempotencyKey,
			ControlReservationID: reservation.ReservationID, TerminalReason: "owner reservation bind failed",
		})
		c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "The owner reservation was committed but its local outcome is unknown; reconcile before retrying."))
		return
	}
	if bound == nil || bound.OperationID != commitment.OperationID || bound.Status == "" {
		c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "The owner reservation receipt is incomplete; reconciliation is required."))
		return
	}
	if bound.Status == "completed" {
		c.JSON(http.StatusOK, gin.H{"data": gin.H{"ticket": bound.Ticket, "operation": bound}})
		return
	}
	if bound.Status != "reserved" {
		c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "The owner reservation is not in a finalizable state; reconcile before retrying."))
		return
	}
	// Control's committed reservation is the cross-plane ordering receipt. A
	// final current-authority read immediately before the owner transaction
	// closes the post-commit revocation window as far as a non-transactional
	// cross-plane boundary can; the owner transaction still independently
	// locks and checks its resource grant.
	if err := h.runActionAuthorityValidator.ValidateRunActionAuthority(c.Request.Context(), decision); err != nil {
		outcome := conversation.TicketOperationOutcomeInput{
			OrgID: decision.OrgID, OperationID: commitment.OperationID, IdempotencyKey: commitment.IdempotencyKey,
			ControlReservationID: reservation.ReservationID,
			TerminalReason:       "Control authority changed before owner commit",
		}
		if errors.Is(err, ErrRunActionAuthorityDenied) {
			if markErr := h.service.MarkAgentTicketOperationCancelled(c.Request.Context(), outcome); markErr != nil {
				c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "Control denied the owner operation but its local cancellation is unresolved."))
				return
			}
			c.JSON(http.StatusForbidden, errorPayload("owner_action_denied", "Control no longer authorizes this ticket operation."))
			return
		}
		outcome.TerminalReason = "Control authority recheck unavailable before owner commit"
		if markErr := h.service.MarkAgentTicketOperationUnknown(c.Request.Context(), outcome); markErr != nil {
			c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "Control authority is unavailable and local reconciliation is unresolved."))
			return
		}
		c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "Control authority could not be rechecked before owner commit; reconcile before retrying."))
		return
	}
	// Do not propagate any caller-selected actor, organization, source, or
	// privileged ticket fields. The non-secret authorization facts are checked
	// inside Conversation Core's owner transaction against its current
	// conversation grant, so a grant revoke cannot race after this HTTP check.
	// The public response remains the same durable owner receipt used by the
	// human action path.
	authorization.ControlReservationID = reservation.ReservationID
	authorization.ActionSchemaHash = decision.ActionSchemaHash
	authorization.PayloadDigest = decision.PayloadDigest
	createInput.AgentActionAuthorization = &authorization
	receipt, err := h.service.CreateTicketOperation(c.Request.Context(), createInput)
	if err != nil {
		deterministic := errors.Is(err, conversation.ErrForbidden) || errors.Is(err, conversation.ErrConflict) || errors.Is(err, conversation.ErrInvalidInput)
		outcome := conversation.TicketOperationOutcomeInput{
			OrgID: decision.OrgID, OperationID: commitment.OperationID, IdempotencyKey: commitment.IdempotencyKey,
			ControlReservationID: reservation.ReservationID,
			TerminalReason:       "owner transaction rejected the committed reservation",
		}
		var markErr error
		if deterministic {
			markErr = h.service.MarkAgentTicketOperationCancelled(c.Request.Context(), outcome)
		} else {
			outcome.TerminalReason = "owner transaction outcome is ambiguous"
			markErr = h.service.MarkAgentTicketOperationUnknown(c.Request.Context(), outcome)
		}
		if markErr != nil {
			c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "The owner transaction outcome is unresolved; reconciliation is required."))
			return
		}
		if deterministic {
			writeServiceError(c, err)
		} else {
			c.JSON(http.StatusConflict, errorPayload("owner_action_unknown", "The ticket operation outcome is unknown; reconcile before retrying."))
		}
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": gin.H{"ticket": receipt.Ticket, "operation": receipt}})
}

func writeOwnerEffectReservationCoordinatorError(c *gin.Context, err error) {
	if errors.Is(err, ErrOwnerEffectReservationDenied) || errors.Is(err, ErrOwnerEffectReservationConflict) {
		c.JSON(http.StatusForbidden, errorPayload("owner_action_denied", "Control does not authorize this ticket operation."))
		return
	}
	c.JSON(http.StatusServiceUnavailable, errorPayload("agent_action_unavailable", "Control owner-effect reservation is unavailable."))
}

func writeRunActionAuthorityError(c *gin.Context, err error) {
	if errors.Is(err, ErrRunActionAuthorityDenied) {
		c.JSON(http.StatusForbidden, errorPayload("owner_action_denied", "Control no longer authorizes this ticket operation."))
		return
	}
	c.JSON(http.StatusServiceUnavailable, errorPayload("agent_action_unavailable", "Control owner-action authority is unavailable."))
}

func validateAgentTicketCreateBody(body agentTicketCreateBody) error {
	for _, value := range []string{body.RunID, body.ControlDecisionToken, body.IdempotencyKey, body.ConversationID} {
		if strings.TrimSpace(value) == "" {
			return conversation.ErrInvalidInput
		}
	}
	if len(strings.TrimSpace(body.RunID)) > 200 || len(strings.TrimSpace(body.ControlDecisionToken)) > 16_384 ||
		len(strings.TrimSpace(body.IdempotencyKey)) > 200 || len(strings.TrimSpace(body.ConversationID)) > 200 ||
		len(strings.TrimSpace(body.Category)) > 80 || len(strings.TrimSpace(body.Intent)) > 120 {
		return conversation.ErrInvalidInput
	}
	if !agentTicketEnum(body.WorkType, "", "customer_case", "internal_work", "incident") ||
		!agentTicketEnum(body.Priority, "", "low", "normal", "high", "urgent") ||
		!agentTicketEnum(body.Severity, "", "low", "medium", "high", "critical") {
		return conversation.ErrInvalidInput
	}
	return nil
}

func agentTicketEnum(value string, allowed ...string) bool {
	value = strings.ToLower(strings.TrimSpace(value))
	for _, candidate := range allowed {
		if value == candidate {
			return true
		}
	}
	return false
}
