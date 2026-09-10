package http

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
)

// ListSpaceActivity answers what this plane can prove about one Space: the
// owner effects that happened under its authority, and the grants that
// authorized them.
//
// # Why the Space gate is the caller's, not this one's
//
// Conversation Core does not know Space membership — Control does, and this
// plane must not grow a second opinion about it. The route therefore scopes by
// the verified organization and the requested `space_ref`, and V3's gateway
// checks Space lifecycle and current Control membership before it ever calls
// here (the same order `GET /spaces/:ref/work` and `/knowledge` use).
//
// That is a NARROWING, not a grant. `GET /api/v1/ticket-operations/:key`
// already answers to any org member for any operation in the org; selecting the
// subset bound to one Space can only ever show less than the caller could
// already see. If this were the only gate on a wider read it would be
// insufficient, and it is not one.
//
// # Space Activity is a projection, never a second source of truth
//
// The response is read-through: no state is stored here, no Space aggregate is
// mirrored, and nothing is recomputed. S2.3's slice 5 asks for the owner event
// to be *correlated* into Space Activity, and correlation through the grant is
// exactly that — the operation ledger stays the record, and this is a view of
// it filtered by the authority that admitted it.
func (h *Handler) ListSpaceActivity(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	spaceRef := strings.TrimSpace(c.Param("space_ref"))
	if spaceRef == "" || len(spaceRef) > 200 {
		c.JSON(http.StatusBadRequest, gin.H{"error": gin.H{
			"code":    "invalid_space_ref",
			"message": "A bounded space_ref is required.",
		}})
		return
	}
	evidence, err := h.service.SpaceActivityEvidence(c.Request.Context(), orgID, spaceRef)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": evidence})
}
