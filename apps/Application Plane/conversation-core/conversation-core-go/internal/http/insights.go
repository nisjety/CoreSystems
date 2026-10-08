package http

import (
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
)

// GetOutcomeReport uses the existing signed gateway delegation and canonical
// organization principal. The query can never select another tenant or user.
func (h *Handler) GetOutcomeReport(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	actor := ""
	switch strings.TrimSpace(c.DefaultQuery("scope", "organization")) {
	case "organization":
	case "me":
		actor = actorUserID(c)
	default:
		c.JSON(http.StatusBadRequest, errorPayload("invalid_scope", "scope must be organization or me"))
		return
	}
	var bounds [2]*time.Time
	for i, key := range []string{"from", "to"} {
		if raw := strings.TrimSpace(c.Query(key)); raw != "" {
			v, err := time.Parse(time.RFC3339Nano, raw)
			if err != nil {
				c.JSON(http.StatusBadRequest, errorPayload("invalid_window", "from and to must be timestamps"))
				return
			}
			bounds[i] = &v
		}
	}
	report, err := h.service.ReadOutcomeReport(c.Request.Context(), orgID, actor, bounds[0], bounds[1])
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": report})
}
