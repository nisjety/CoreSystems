package http

import (
	"net/http"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/conversation"
	"github.com/gin-gonic/gin"
)

func (h *Handler) GetConversationCSATPreference(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	preference, err := h.service.GetConversationCSATPreference(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": preference})
}

func (h *Handler) PatchConversationCSATPreference(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body struct {
		OptedIn *bool `json:"opted_in"`
	}
	if err := c.ShouldBindJSON(&body); err != nil || body.OptedIn == nil {
		writeServiceError(c, conversation.ErrInvalidInput)
		return
	}
	preference, err := h.service.SetConversationCSATPreference(c.Request.Context(), conversation.CSATPreferenceInput{OrgID: orgID, ConversationID: c.Param("id"), ActorUserID: actorUserID(c), OptedIn: *body.OptedIn})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": preference})
}

func (h *Handler) GetTicketCSATOutcome(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	outcome, err := h.service.GetTicketCSATOutcome(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": outcome})
}

func (h *Handler) PutTicketCSATOutcome(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body struct {
		Score *int `json:"score"`
	}
	if err := c.ShouldBindJSON(&body); err != nil || body.Score == nil {
		writeServiceError(c, conversation.ErrInvalidInput)
		return
	}
	outcome, err := h.service.RecordTicketCSATOutcome(c.Request.Context(), conversation.TicketCSATOutcomeInput{
		OrgID: orgID, TicketID: c.Param("id"), Score: *body.Score, RecordedBy: actorUserID(c),
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": outcome})
}

func (h *Handler) GetCSATScorecard(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	scorecard, err := h.service.GetCSATScorecard(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": scorecard})
}
