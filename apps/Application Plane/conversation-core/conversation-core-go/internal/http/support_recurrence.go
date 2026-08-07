package http

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

// GetSupportRecurrenceCandidates is the gateway-facing endpoint behind the
// semantic support-recurrence "similarity candidates" preview
// (verevon-inbox.md's design gate). Permission, active-membership, and ZDR
// checks all happen in the gateway before this is ever reached — this
// handler only re-derives the anchor ticket from the caller's own verified
// org (never trusts the path param as a bare lookup key) via
// Service.FindSupportRecurrenceCandidates.
func (h *Handler) GetSupportRecurrenceCandidates(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	result, err := h.service.FindSupportRecurrenceCandidates(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": result})
}
