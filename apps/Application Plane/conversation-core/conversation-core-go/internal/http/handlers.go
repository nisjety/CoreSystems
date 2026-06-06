package http

import (
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
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
	BodyText   string `json:"body_text"`
	Body       string `json:"body"`
	BodyHTML   string `json:"body_html"`
	Internal   bool   `json:"internal"`
	ActorName  string `json:"actor_name"`
	ActorEmail string `json:"actor_email"`
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

func (h *Handler) IngestEvent(c *gin.Context) {
	var event conversation.InboundEvent
	if err := c.ShouldBindJSON(&event); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
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
	text := strings.TrimSpace(body.BodyText)
	if text == "" {
		text = strings.TrimSpace(body.Body)
	}
	message, err := h.service.AddMessage(c.Request.Context(), conversation.AddMessageInput{
		OrgID:          orgID,
		ConversationID: c.Param("id"),
		ActorUserID:    actorUserID(c),
		ActorName:      body.ActorName,
		ActorEmail:     body.ActorEmail,
		BodyText:       text,
		BodyHTML:       body.BodyHTML,
		Internal:       forceInternal || body.Internal,
		Direction:      conversation.DirectionOutbound,
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
	orgID := strings.TrimSpace(c.GetHeader("x-org-id"))
	if orgID == "" {
		orgID = strings.TrimSpace(c.Query("org_id"))
	}
	if orgID == "" {
		orgID = strings.TrimSpace(c.Query("orgId"))
	}
	if orgID == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_org_id", "x-org-id is required."))
		return ""
	}
	return orgID
}

func actorUserID(c *gin.Context) string {
	userID := strings.TrimSpace(c.GetHeader("x-user-id"))
	if userID == "" {
		return "internal-service"
	}
	return userID
}

func writeServiceError(c *gin.Context, err error) {
	switch {
	case errors.Is(err, conversation.ErrNotFound):
		c.JSON(http.StatusNotFound, errorPayload("not_found", "Conversation resource was not found."))
	case conversation.IsInvalidInput(err):
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", err.Error()))
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
