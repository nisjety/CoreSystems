package http

import (
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/gin-gonic/gin"
	"net/http"
)

type conversationDraftBody struct {
	BodyText string `json:"body_text"`
	Internal bool   `json:"internal"`
}

func (h *Handler) GetConversationDraft(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	draft, err := h.service.GetConversationDraft(c.Request.Context(), orgID, c.Param("id"), actorUserID(c))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": draft})
}
func (h *Handler) UpsertConversationDraft(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body conversationDraftBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	draft, err := h.service.UpsertConversationDraft(c.Request.Context(), conversation.ConversationDraftInput{OrgID: orgID, ConversationID: c.Param("id"), UserID: actorUserID(c), BodyText: body.BodyText, Internal: body.Internal})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": draft})
}
func (h *Handler) DeleteConversationDraft(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	if err := h.service.DeleteConversationDraft(c.Request.Context(), orgID, c.Param("id"), actorUserID(c)); err != nil {
		writeServiceError(c, err)
		return
	}
	c.Status(http.StatusNoContent)
}
