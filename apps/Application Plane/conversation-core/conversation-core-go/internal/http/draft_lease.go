package http

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

func (h *Handler) GetDraftLease(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	lease, err := h.service.GetDraftLease(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": lease})
}

func (h *Handler) ClaimDraftLease(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	lease, err := h.service.ClaimDraftLease(c.Request.Context(), orgID, c.Param("id"), actorUserID(c))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": lease})
}

func (h *Handler) ReleaseDraftLease(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	if err := h.service.ReleaseDraftLease(c.Request.Context(), orgID, c.Param("id"), actorUserID(c)); err != nil {
		writeServiceError(c, err)
		return
	}
	c.Status(http.StatusNoContent)
}
