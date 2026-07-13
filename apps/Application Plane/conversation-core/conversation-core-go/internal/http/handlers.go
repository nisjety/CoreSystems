package http

import (
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
	"github.com/gin-gonic/gin"
)

type Handler struct {
	cfg     *config.Config
	service *conversation.Service
}

func NewHandler(cfg *config.Config, service *conversation.Service) *Handler {
	return &Handler{cfg: cfg, service: service}
}

type addMessageBody struct {
	BodyText       string `json:"body_text"`
	Body           string `json:"body"`
	BodyHTML       string `json:"body_html"`
	Internal       bool   `json:"internal"`
	IdempotencyKey string `json:"idempotency_key"`
}

type statusBody struct {
	Status  string `json:"status"`
	StateID *int   `json:"state_id"`
}

type assignmentBody struct {
	AssigneeUserID string `json:"assignee_user_id"`
	AssigneeName   string `json:"assignee_name"`
	OwnerID        *int   `json:"owner_id"`
}

type tagBody struct {
	Tag  string   `json:"tag"`
	Tags []string `json:"tags"`
}

type reviewBody struct {
	Decision string `json:"decision"`
	Comment  string `json:"comment"`
}

type createAIActionBody struct {
	ConversationID string         `json:"conversation_id"`
	Kind           string         `json:"kind"`
	Payload        map[string]any `json:"payload"`
}

type createTicketBody struct {
	ConversationID      string   `json:"conversation_id"`
	Status              string   `json:"status"`
	Priority            string   `json:"priority"`
	Severity            string   `json:"severity"`
	Category            string   `json:"category"`
	Intent              string   `json:"intent"`
	AssigneeUserID      string   `json:"assignee_user_id"`
	AssigneeName        string   `json:"assignee_name"`
	TeamID              string   `json:"team_id"`
	TeamName            string   `json:"team_name"`
	DueAt               string   `json:"due_at"`
	Source              string   `json:"source"`
	AIConfidence        float64  `json:"ai_confidence"`
	AIReason            string   `json:"ai_reason"`
	CreatedBy           string   `json:"created_by"`
	WaitingSince        string   `json:"waiting_since"`
	LastCustomerReplyAt string   `json:"last_customer_reply_at"`
	FirstResponseAt     string   `json:"first_response_at"`
	ResolvedAt          string   `json:"resolved_at"`
	SnoozedUntil        string   `json:"snoozed_until"`
	SLAPolicyID         string   `json:"sla_policy_id"`
	EscalationAt        string   `json:"escalation_at"`
	Labels              []string `json:"labels"`
}

type patchTicketBody struct {
	Status              *string   `json:"status"`
	Priority            *string   `json:"priority"`
	Severity            *string   `json:"severity"`
	Category            *string   `json:"category"`
	Intent              *string   `json:"intent"`
	AssigneeUserID      *string   `json:"assignee_user_id"`
	AssigneeName        *string   `json:"assignee_name"`
	TeamID              *string   `json:"team_id"`
	TeamName            *string   `json:"team_name"`
	DueAt               *string   `json:"due_at"`
	Source              *string   `json:"source"`
	AIConfidence        *float64  `json:"ai_confidence"`
	AIReason            *string   `json:"ai_reason"`
	WaitingSince        *string   `json:"waiting_since"`
	LastCustomerReplyAt *string   `json:"last_customer_reply_at"`
	FirstResponseAt     *string   `json:"first_response_at"`
	ResolvedAt          *string   `json:"resolved_at"`
	SnoozedUntil        *string   `json:"snoozed_until"`
	SLAPolicyID         *string   `json:"sla_policy_id"`
	EscalationAt        *string   `json:"escalation_at"`
	Labels              *[]string `json:"labels"`
}

type linkTicketResourceBody struct {
	LinkType     string         `json:"link_type"`
	ResourceKind string         `json:"resource_kind"`
	ResourceID   string         `json:"resource_id"`
	ResourceURL  string         `json:"resource_url"`
	Label        string         `json:"label"`
	Metadata     map[string]any `json:"metadata"`
}

type ticketClassificationBody struct {
	Outcome            string         `json:"outcome"`
	Confidence         float64        `json:"confidence"`
	Reason             string         `json:"reason"`
	SuggestedFields    map[string]any `json:"suggested_fields"`
	EvidenceMessageIDs []string       `json:"evidence_message_ids"`
}

type ticketViewBody struct {
	Name         *string         `json:"name"`
	Scope        *string         `json:"scope"`
	OwnerUserID  *string         `json:"owner_user_id"`
	TeamID       *string         `json:"team_id"`
	Visibility   *string         `json:"visibility"`
	Filter       *map[string]any `json:"filter"`
	Sort         *map[string]any `json:"sort"`
	GroupBy      *string         `json:"group_by"`
	SidebarOrder *int            `json:"sidebar_order"`
}

type ticketMacroBody struct {
	Name        *string         `json:"name"`
	Description *string         `json:"description"`
	Visibility  *string         `json:"visibility"`
	TeamID      *string         `json:"team_id"`
	Active      *bool           `json:"active"`
	Actions     *map[string]any `json:"actions"`
	Conditions  *map[string]any `json:"conditions"`
}

type ticketAutomationRuleBody struct {
	Name       *string         `json:"name"`
	EventName  *string         `json:"event_name"`
	Active     *bool           `json:"active"`
	Conditions *map[string]any `json:"conditions"`
	Actions    *map[string]any `json:"actions"`
}

type slaPolicyBody struct {
	Name                 *string         `json:"name"`
	Active               *bool           `json:"active"`
	Conditions           *map[string]any `json:"conditions"`
	CalendarRef          *string         `json:"calendar_ref"`
	FirstResponseMinutes *int            `json:"first_response_minutes"`
	NextResponseMinutes  *int            `json:"next_response_minutes"`
	ResolutionMinutes    *int            `json:"resolution_minutes"`
}

type createTicketChecklistBody struct {
	Name       string   `json:"name"`
	TemplateID string   `json:"template_id"`
	Items      []string `json:"items"`
}

type patchTicketChecklistItemBody struct {
	Completed bool `json:"completed"`
}

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"status": "ok", "service": h.cfg.ServiceName})
}

func (h *Handler) ListInboxes(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	inboxes, err := h.service.ListInboxes(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": inboxes})
}

func (h *Handler) ListInboxQueue(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	filter := listFilterFromRequest(c, orgID)
	filter.InboxID = c.Param("id")
	h.listConversations(c, filter)
}

func (h *Handler) ListConversations(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	h.listConversations(c, listFilterFromRequest(c, orgID))
}

func (h *Handler) SearchConversations(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body struct {
		Query string `json:"query"`
		Limit int    `json:"limit"`
	}
	_ = c.ShouldBindJSON(&body)
	filter := listFilterFromRequest(c, orgID)
	filter.Query = body.Query
	if body.Limit > 0 {
		filter.Limit = body.Limit
	}
	h.listConversations(c, filter)
}

func (h *Handler) listConversations(c *gin.Context, filter conversation.ListFilter) {
	conversations, err := h.service.ListConversations(c.Request.Context(), filter)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": conversations, "meta": gin.H{"limit": filter.Limit, "has_next": false, "next_cursor": nil}})
}

func (h *Handler) GetConversation(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	detail, err := h.service.GetConversation(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": detail})
}

func (h *Handler) ListTickets(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	limit, _ := strconv.Atoi(c.DefaultQuery("limit", "50"))
	tickets, err := h.service.ListTickets(c.Request.Context(), conversation.TicketListFilter{
		OrgID:    orgID,
		Queue:    c.Query("queue"),
		Status:   c.Query("status"),
		Assigned: c.Query("assigned"),
		TeamID:   c.Query("team"),
		Label:    c.Query("label"),
		Priority: c.Query("priority"),
		Severity: c.Query("severity"),
		SLAState: c.Query("sla_state"),
		Query:    c.Query("q"),
		Limit:    limit,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": tickets, "meta": gin.H{"limit": limit, "has_next": false, "next_cursor": nil}})
}

func (h *Handler) CreateTicket(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body createTicketBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	dueAt, err := parseOptionalTime(body.DueAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "due_at must be RFC3339 when provided."))
		return
	}
	waitingSince, err := parseOptionalTime(body.WaitingSince)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "waiting_since must be RFC3339 when provided."))
		return
	}
	lastCustomerReplyAt, err := parseOptionalTime(body.LastCustomerReplyAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "last_customer_reply_at must be RFC3339 when provided."))
		return
	}
	firstResponseAt, err := parseOptionalTime(body.FirstResponseAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "first_response_at must be RFC3339 when provided."))
		return
	}
	resolvedAt, err := parseOptionalTime(body.ResolvedAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "resolved_at must be RFC3339 when provided."))
		return
	}
	snoozedUntil, err := parseOptionalTime(body.SnoozedUntil)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "snoozed_until must be RFC3339 when provided."))
		return
	}
	escalationAt, err := parseOptionalTime(body.EscalationAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "escalation_at must be RFC3339 when provided."))
		return
	}
	ticket, err := h.service.CreateTicket(c.Request.Context(), conversation.CreateTicketInput{
		OrgID:               orgID,
		ConversationID:      body.ConversationID,
		Status:              body.Status,
		Priority:            body.Priority,
		Severity:            body.Severity,
		Category:            body.Category,
		Intent:              body.Intent,
		AssigneeUserID:      body.AssigneeUserID,
		AssigneeName:        body.AssigneeName,
		TeamID:              body.TeamID,
		TeamName:            body.TeamName,
		DueAt:               dueAt,
		Source:              "manual",
		AIConfidence:        0,
		AIReason:            "",
		CreatedBy:           actorUserID(c),
		WaitingSince:        waitingSince,
		LastCustomerReplyAt: lastCustomerReplyAt,
		FirstResponseAt:     firstResponseAt,
		ResolvedAt:          resolvedAt,
		SnoozedUntil:        snoozedUntil,
		SLAPolicyID:         body.SLAPolicyID,
		EscalationAt:        escalationAt,
		Labels:              body.Labels,
		ActorUserID:         actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": ticket})
}

func (h *Handler) GetTicket(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	ticket, err := h.service.GetTicket(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": ticket})
}

func (h *Handler) PatchTicket(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body patchTicketBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	dueAt, err := parseOptionalTimePtr(body.DueAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "due_at must be RFC3339 when provided."))
		return
	}
	waitingSince, err := parseOptionalTimePtr(body.WaitingSince)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "waiting_since must be RFC3339 when provided."))
		return
	}
	lastCustomerReplyAt, err := parseOptionalTimePtr(body.LastCustomerReplyAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "last_customer_reply_at must be RFC3339 when provided."))
		return
	}
	firstResponseAt, err := parseOptionalTimePtr(body.FirstResponseAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "first_response_at must be RFC3339 when provided."))
		return
	}
	resolvedAt, err := parseOptionalTimePtr(body.ResolvedAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "resolved_at must be RFC3339 when provided."))
		return
	}
	snoozedUntil, err := parseOptionalTimePtr(body.SnoozedUntil)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "snoozed_until must be RFC3339 when provided."))
		return
	}
	escalationAt, err := parseOptionalTimePtr(body.EscalationAt)
	if err != nil {
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", "escalation_at must be RFC3339 when provided."))
		return
	}
	ticket, err := h.service.UpdateTicket(c.Request.Context(), conversation.UpdateTicketInput{
		OrgID:               orgID,
		TicketID:            c.Param("id"),
		Status:              body.Status,
		Priority:            body.Priority,
		Severity:            body.Severity,
		Category:            body.Category,
		Intent:              body.Intent,
		AssigneeUserID:      body.AssigneeUserID,
		AssigneeName:        body.AssigneeName,
		TeamID:              body.TeamID,
		TeamName:            body.TeamName,
		DueAt:               dueAt,
		Source:              nil,
		AIConfidence:        nil,
		AIReason:            nil,
		WaitingSince:        waitingSince,
		LastCustomerReplyAt: lastCustomerReplyAt,
		FirstResponseAt:     firstResponseAt,
		ResolvedAt:          resolvedAt,
		SnoozedUntil:        snoozedUntil,
		SLAPolicyID:         body.SLAPolicyID,
		EscalationAt:        escalationAt,
		Labels:              body.Labels,
		ActorUserID:         actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": ticket})
}

func (h *Handler) LinkTicketResource(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body linkTicketResourceBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	link, err := h.service.LinkTicketResource(c.Request.Context(), conversation.LinkTicketResourceInput{
		OrgID:           orgID,
		TicketID:        c.Param("id"),
		LinkType:        body.LinkType,
		ResourceKind:    body.ResourceKind,
		ResourceID:      body.ResourceID,
		ResourceURL:     body.ResourceURL,
		Label:           body.Label,
		Metadata:        body.Metadata,
		CreatedByUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": link})
}

func (h *Handler) ClassifyConversationForTicket(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body ticketClassificationBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	classification, err := h.service.RecordTicketClassification(c.Request.Context(), conversation.TicketClassificationInput{
		OrgID:              orgID,
		ConversationID:     c.Param("id"),
		Outcome:            body.Outcome,
		Confidence:         body.Confidence,
		Reason:             body.Reason,
		SuggestedFields:    body.SuggestedFields,
		EvidenceMessageIDs: body.EvidenceMessageIDs,
		ActorUserID:        actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": classification})
}

func (h *Handler) ListTicketViews(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	items, err := h.service.ListTicketViews(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": items})
}

func (h *Handler) CreateTicketView(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body ticketViewBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	item, err := h.service.CreateTicketView(c.Request.Context(), conversation.CreateTicketViewInput{
		OrgID:        orgID,
		Name:         stringValue(body.Name),
		Scope:        stringValue(body.Scope),
		OwnerUserID:  stringValue(body.OwnerUserID),
		TeamID:       stringValue(body.TeamID),
		Visibility:   stringValue(body.Visibility),
		Filter:       mapValue(body.Filter),
		Sort:         mapValue(body.Sort),
		GroupBy:      stringValue(body.GroupBy),
		SidebarOrder: intValue(body.SidebarOrder),
		ActorUserID:  actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": item})
}

func (h *Handler) PatchTicketView(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body ticketViewBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	item, err := h.service.UpdateTicketView(c.Request.Context(), conversation.UpdateTicketViewInput{
		OrgID:        orgID,
		ID:           c.Param("id"),
		Name:         body.Name,
		Scope:        body.Scope,
		OwnerUserID:  body.OwnerUserID,
		TeamID:       body.TeamID,
		Visibility:   body.Visibility,
		Filter:       body.Filter,
		Sort:         body.Sort,
		GroupBy:      body.GroupBy,
		SidebarOrder: body.SidebarOrder,
		ActorUserID:  actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": item})
}

func (h *Handler) ListTicketMacros(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	items, err := h.service.ListTicketMacros(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": items})
}

func (h *Handler) CreateTicketMacro(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body ticketMacroBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	active := true
	if body.Active != nil {
		active = *body.Active
	}
	item, err := h.service.CreateTicketMacro(c.Request.Context(), conversation.CreateTicketMacroInput{
		OrgID:       orgID,
		Name:        stringValue(body.Name),
		Description: stringValue(body.Description),
		Visibility:  stringValue(body.Visibility),
		TeamID:      stringValue(body.TeamID),
		Active:      active,
		Actions:     mapValue(body.Actions),
		Conditions:  mapValue(body.Conditions),
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": item})
}

func (h *Handler) PatchTicketMacro(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body ticketMacroBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	item, err := h.service.UpdateTicketMacro(c.Request.Context(), conversation.UpdateTicketMacroInput{
		OrgID:       orgID,
		ID:          c.Param("id"),
		Name:        body.Name,
		Description: body.Description,
		Visibility:  body.Visibility,
		TeamID:      body.TeamID,
		Active:      body.Active,
		Actions:     body.Actions,
		Conditions:  body.Conditions,
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": item})
}

func (h *Handler) RunTicketMacro(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	result, err := h.service.RunTicketMacro(c.Request.Context(), conversation.TicketMacroRunInput{
		OrgID:       orgID,
		TicketID:    c.Param("id"),
		MacroID:     c.Param("macro_id"),
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": result})
}

func (h *Handler) ListTicketAutomationRules(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	items, err := h.service.ListTicketAutomationRules(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": items})
}

func (h *Handler) CreateTicketAutomationRule(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body ticketAutomationRuleBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	active := true
	if body.Active != nil {
		active = *body.Active
	}
	item, err := h.service.CreateTicketAutomationRule(c.Request.Context(), conversation.CreateTicketAutomationRuleInput{
		OrgID:       orgID,
		Name:        stringValue(body.Name),
		EventName:   stringValue(body.EventName),
		Active:      active,
		Conditions:  mapValue(body.Conditions),
		Actions:     mapValue(body.Actions),
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": item})
}

func (h *Handler) PatchTicketAutomationRule(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body ticketAutomationRuleBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	item, err := h.service.UpdateTicketAutomationRule(c.Request.Context(), conversation.UpdateTicketAutomationRuleInput{
		OrgID:       orgID,
		ID:          c.Param("id"),
		Name:        body.Name,
		EventName:   body.EventName,
		Active:      body.Active,
		Conditions:  body.Conditions,
		Actions:     body.Actions,
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": item})
}

func (h *Handler) ListSLAPolicies(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	items, err := h.service.ListSLAPolicies(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": items})
}

func (h *Handler) CreateSLAPolicy(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body slaPolicyBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	active := true
	if body.Active != nil {
		active = *body.Active
	}
	item, err := h.service.CreateSLAPolicy(c.Request.Context(), conversation.CreateSLAPolicyInput{
		OrgID:                orgID,
		Name:                 stringValue(body.Name),
		Active:               active,
		Conditions:           mapValue(body.Conditions),
		CalendarRef:          stringValue(body.CalendarRef),
		FirstResponseMinutes: intValue(body.FirstResponseMinutes),
		NextResponseMinutes:  intValue(body.NextResponseMinutes),
		ResolutionMinutes:    intValue(body.ResolutionMinutes),
		ActorUserID:          actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": item})
}

func (h *Handler) PatchSLAPolicy(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body slaPolicyBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	item, err := h.service.UpdateSLAPolicy(c.Request.Context(), conversation.UpdateSLAPolicyInput{
		OrgID:                orgID,
		ID:                   c.Param("id"),
		Name:                 body.Name,
		Active:               body.Active,
		Conditions:           body.Conditions,
		CalendarRef:          body.CalendarRef,
		FirstResponseMinutes: body.FirstResponseMinutes,
		NextResponseMinutes:  body.NextResponseMinutes,
		ResolutionMinutes:    body.ResolutionMinutes,
		ActorUserID:          actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": item})
}

func (h *Handler) CreateTicketChecklist(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body createTicketChecklistBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	checklist, err := h.service.CreateTicketChecklist(c.Request.Context(), conversation.CreateTicketChecklistInput{
		OrgID:           orgID,
		TicketID:        c.Param("id"),
		Name:            body.Name,
		TemplateID:      body.TemplateID,
		Items:           body.Items,
		CreatedByUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": checklist})
}

func (h *Handler) PatchTicketChecklistItem(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body patchTicketChecklistItemBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	checklist, err := h.service.UpdateTicketChecklistItem(c.Request.Context(), conversation.UpdateTicketChecklistItemInput{
		OrgID:       orgID,
		TicketID:    c.Param("id"),
		ChecklistID: c.Param("checklist_id"),
		ItemID:      c.Param("item_id"),
		Completed:   body.Completed,
		ActorUserID: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": checklist})
}

func (h *Handler) IngestEvent(c *gin.Context) {
	var event conversation.InboundEvent
	if err := c.ShouldBindJSON(&event); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	principal, ok := delegation.PrincipalFromContext(c.Request.Context())
	if !ok || principal.ServiceID != "conversation-ingest" || strings.TrimSpace(event.OrgID) != principal.OrganizationID {
		c.JSON(http.StatusForbidden, errorPayload("forbidden", "event organization does not match verified delegation scope"))
		return
	}
	result, err := h.service.IngestEvent(c.Request.Context(), event)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusAccepted, gin.H{"data": result})
}

func (h *Handler) AddMessage(c *gin.Context) {
	h.addMessage(c, false)
}

func (h *Handler) AddNote(c *gin.Context) {
	h.addMessage(c, true)
}

func (h *Handler) addMessage(c *gin.Context, forceInternal bool) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body addMessageBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	if !forceInternal && body.Internal {
		c.JSON(http.StatusUnprocessableEntity, errorPayload(
			"validation_error",
			"internal is not accepted on the messages route. Use /notes for internal notes.",
		))
		return
	}
	text := strings.TrimSpace(body.BodyText)
	if text == "" {
		text = strings.TrimSpace(body.Body)
	}
	actorName, actorEmail := trustedMessageActor(c)
	message, err := h.service.AddMessage(c.Request.Context(), conversation.AddMessageInput{
		OrgID:          orgID,
		ConversationID: c.Param("id"),
		ActorUserID:    actorUserID(c),
		ActorName:      actorName,
		ActorEmail:     actorEmail,
		BodyText:       text,
		BodyHTML:       body.BodyHTML,
		Internal:       forceInternal,
		Direction:      conversation.DirectionOutbound,
		IdempotencyKey: body.IdempotencyKey,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": message})
}

func (h *Handler) UpdateStatus(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body statusBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	status := body.Status
	if status == "" && body.StateID != nil {
		status = statusFromStateID(*body.StateID)
	}
	detail, err := h.service.UpdateStatus(c.Request.Context(), conversation.StatusUpdate{
		OrgID:          orgID,
		ConversationID: c.Param("id"),
		Status:         status,
		ActorUserID:    actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": detail})
}

func (h *Handler) UpdateAssignment(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body assignmentBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	assigneeUserID := body.AssigneeUserID
	if assigneeUserID == "" && body.OwnerID != nil {
		assigneeUserID = strconv.Itoa(*body.OwnerID)
	}
	detail, err := h.service.UpdateAssignment(c.Request.Context(), conversation.AssignmentUpdate{
		OrgID:          orgID,
		ConversationID: c.Param("id"),
		AssigneeUserID: assigneeUserID,
		AssigneeName:   body.AssigneeName,
		ActorUserID:    actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": detail})
}

func (h *Handler) AddTag(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body tagBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	tag := body.Tag
	if tag == "" && len(body.Tags) > 0 {
		tag = body.Tags[0]
	}
	detail, err := h.service.AddTag(c.Request.Context(), orgID, c.Param("id"), tag, actorUserID(c))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": detail})
}

func (h *Handler) RemoveTag(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	detail, err := h.service.RemoveTag(c.Request.Context(), orgID, c.Param("id"), c.Param("tag"), actorUserID(c))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": detail})
}

// CreateAIAction queues a model-proposed action (e.g. a draft.reply) into the
// HITL review queue. IDOR-clean: org is taken from the header context only and
// the actor is the authenticated user — the request body org is never trusted.
func (h *Handler) CreateAIAction(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body createAIActionBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	action, err := h.service.CreateAIAction(c.Request.Context(), conversation.CreateAIActionInput{
		OrgID:          orgID,
		ConversationID: body.ConversationID,
		Kind:           body.Kind,
		Payload:        body.Payload,
		CreatedBy:      actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": action})
}

func (h *Handler) ListAIActions(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	limit, _ := strconv.Atoi(c.DefaultQuery("limit", "50"))
	conversationID := strings.TrimSpace(c.Query("conversation_id"))
	if conversationID == "" {
		conversationID = strings.TrimSpace(c.Query("conversationId"))
	}
	actions, err := h.service.ListAIActions(c.Request.Context(), conversation.AIActionListFilter{
		OrgID:          orgID,
		Status:         c.Query("status"),
		ConversationID: conversationID,
		Limit:          limit,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": actions, "meta": gin.H{"limit": limit, "has_next": false, "next_cursor": nil}})
}

func (h *Handler) ReviewAIAction(c *gin.Context) {
	h.reviewAIAction(c, "")
}

func (h *Handler) ApproveAIAction(c *gin.Context) {
	h.reviewAIAction(c, "approved")
}

func (h *Handler) RejectAIAction(c *gin.Context) {
	h.reviewAIAction(c, "rejected")
}

func (h *Handler) reviewAIAction(c *gin.Context, forcedDecision string) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body reviewBody
	_ = c.ShouldBindJSON(&body)
	if forcedDecision != "" {
		body.Decision = forcedDecision
	}
	if err := h.service.ReviewAIAction(c.Request.Context(), conversation.AIActionReview{
		OrgID:      orgID,
		AIActionID: c.Param("id"),
		ReviewerID: actorUserID(c),
		Decision:   body.Decision,
		Comment:    body.Comment,
		OccurredAt: time.Now().UTC(),
	}); err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"ok": true}})
}

func listFilterFromRequest(c *gin.Context, orgID string) conversation.ListFilter {
	limit, _ := strconv.Atoi(c.DefaultQuery("limit", "50"))
	filter := conversation.ListFilter{
		OrgID:    orgID,
		Status:   c.Query("status"),
		Assigned: c.Query("assigned"),
		Channel:  c.Query("channel"),
		Query:    c.Query("q"),
		Limit:    limit,
	}
	if state := c.Query("state"); filter.Status == "" && state != "" {
		filter.Status = statusFromTab(state)
	}
	return filter
}

func requireOrgID(c *gin.Context) string {
	principal, ok := delegation.PrincipalFromContext(c.Request.Context())
	orgID := strings.TrimSpace(principal.OrganizationID)
	if !ok || orgID == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_org_id", "verified organization scope is required."))
		return ""
	}
	return orgID
}

func parseOptionalTime(value string) (*time.Time, error) {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil, nil
	}
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return nil, err
	}
	utc := parsed.UTC()
	return &utc, nil
}

func parseOptionalTimePtr(value *string) (*time.Time, error) {
	if value == nil {
		return nil, nil
	}
	return parseOptionalTime(*value)
}

func actorUserID(c *gin.Context) string {
	principal, ok := delegation.PrincipalFromContext(c.Request.Context())
	if !ok {
		return ""
	}
	return strings.TrimSpace(principal.UserID)
}

func trustedMessageActor(c *gin.Context) (string, string) {
	// The v2 delegation contract binds the stable Control user id, not display
	// attributes. Persist that verified id rather than caller-controlled JSON or
	// unsigned x-user-name/x-user-email values.
	return actorUserID(c), ""
}

func stringValue(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}

func intValue(value *int) int {
	if value == nil {
		return 0
	}
	return *value
}

func mapValue(value *map[string]any) map[string]any {
	if value == nil {
		return map[string]any{}
	}
	return *value
}

func writeServiceError(c *gin.Context, err error) {
	switch {
	case errors.Is(err, conversation.ErrNotFound):
		c.JSON(http.StatusNotFound, errorPayload("not_found", "Conversation resource was not found."))
	case errors.Is(err, conversation.ErrConflict):
		c.JSON(http.StatusConflict, errorPayload("conflict", "Conversation resource already exists."))
	case conversation.IsInvalidInput(err):
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", err.Error()))
	case errors.Is(err, conversation.ErrDeliveryUnknown):
		c.JSON(http.StatusConflict, errorPayload("delivery_unknown", "The reply may have been submitted, but its outcome is unknown. Do not retry automatically; reconciliation is required."))
	case errors.Is(err, conversation.ErrSendFailed):
		// The reply was attempted but the customer channel did not accept it.
		// 502 (not 201) so the Inbox surfaces a real failure instead of a phantom
		// "Reply sent" for a message that was never delivered or persisted.
		c.JSON(http.StatusBadGateway, errorPayload("send_failed", "The reply could not be delivered to the customer channel and was not sent."))
	case errors.Is(err, conversation.ErrDeliveryUnavailable):
		c.JSON(http.StatusServiceUnavailable, errorPayload("delivery_unavailable", "The customer channel is not configured for outbound delivery. The reply was not sent or stored."))
	default:
		c.JSON(http.StatusInternalServerError, errorPayload("internal_error", "Internal error."))
	}
}

func errorPayload(code, message string) gin.H {
	return gin.H{"error": gin.H{"code": code, "message": message}}
}

func statusFromStateID(id int) string {
	switch id {
	case 1, 2:
		return conversation.StatusOpen
	case 4:
		return conversation.StatusSolved
	case 6:
		return conversation.StatusPending
	default:
		return ""
	}
}

func statusFromTab(tab string) string {
	switch strings.ToLower(strings.TrimSpace(tab)) {
	case "open", "new":
		return conversation.StatusOpen
	case "pending":
		return conversation.StatusPending
	case "solved", "closed":
		return conversation.StatusSolved
	default:
		return ""
	}
}
