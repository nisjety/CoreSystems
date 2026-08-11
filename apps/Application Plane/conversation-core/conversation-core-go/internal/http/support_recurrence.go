package http

import (
	"net/http"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
	"github.com/gin-gonic/gin"
)

// GetSupportRecurrenceCandidates is the gateway-facing endpoint behind the
// semantic support-recurrence "similarity candidates" preview
// (verevon-inbox.md's design gate). The gateway may provide a UX preflight, but
// Conversation Core independently resolves policy from Org Core before reading
// its semantic corpus so a direct or future internal caller cannot bypass it.
func (h *Handler) GetSupportRecurrenceCandidates(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	if !h.requireSupportRecurrencePolicy(c, orgID) {
		return
	}
	result, err := h.service.FindSupportRecurrenceCandidates(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": result})
}

func (h *Handler) requireSupportRecurrencePolicy(c *gin.Context, orgID string) bool {
	principal, ok := delegation.PrincipalFromContext(c.Request.Context())
	role := ""
	if ok {
		role = strings.TrimSpace(principal.Role)
	}
	if h.supportPolicy == nil || role == "" {
		c.JSON(http.StatusServiceUnavailable, errorPayload(
			"support_recurrence_policy_unavailable",
			"Similarity candidates are unavailable until the organization policy can be verified.",
		))
		return false
	}
	policy, err := h.supportPolicy.SupportPolicy(c.Request.Context(), orgID, role)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload(
			"support_recurrence_policy_unavailable",
			"Similarity candidates are unavailable until the organization policy can be verified.",
		))
		return false
	}
	if policy.ZDREnabled {
		c.JSON(http.StatusPreconditionFailed, errorPayload(
			"zdr_recurrence_forbidden",
			"Similarity candidates are not available while Zero Data Retention is enabled.",
		))
		return false
	}
	if !policy.RecurrenceAllowed {
		c.JSON(http.StatusForbidden, errorPayload(
			"support_recurrence_permission_required",
			"Your role does not have permission to view similarity candidates.",
		))
		return false
	}
	return true
}
