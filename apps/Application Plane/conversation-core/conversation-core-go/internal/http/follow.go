package http

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

func (h *Handler) GetConversationFollow(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	follow, err := h.service.GetConversationFollow(c.Request.Context(), orgID, c.Param("id"), actorUserID(c))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": follow})
}

func (h *Handler) FollowConversation(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	follow, err := h.service.FollowConversation(c.Request.Context(), orgID, c.Param("id"), actorUserID(c))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": follow})
}

func (h *Handler) UnfollowConversation(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	if err := h.service.UnfollowConversation(c.Request.Context(), orgID, c.Param("id"), actorUserID(c)); err != nil {
		writeServiceError(c, err)
		return
	}
	c.Status(http.StatusNoContent)
}
