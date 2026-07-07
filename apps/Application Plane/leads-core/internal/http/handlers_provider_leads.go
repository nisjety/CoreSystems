package http

import (
	"log"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/providerleads"
)

// SetProviderLeads wires the optional provider-lead sync surface. When the
// syncer is not configured (no actions-gateway URL), the routes answer 503
// honestly instead of pretending a sync ran.
func (h *Handler) SetProviderLeads(syncer *providerleads.Syncer, repo providerleads.Repository) {
	h.providerLeadSyncer = syncer
	h.providerLeadRepo = repo
}

type providerLeadSyncBody struct {
	OrganizationID string `json:"organization_id"`
	// Owner overrides the lead-forms owner URN for every connection in this
	// run (escape hatch when providerContext carries no owner URN).
	Owner string `json:"owner"`
}

// SyncProviderLeads is the manual internal trigger
// (POST /internal/sync/provider-leads, internal-key gated). Body is optional.
func (h *Handler) SyncProviderLeads(c *gin.Context) {
	if h.providerLeadSyncer == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("not_configured", "Provider lead sync is not configured (INTEGRATION_CORE_URL)."))
		return
	}
	var body providerLeadSyncBody
	if c.Request.ContentLength > 0 {
		if err := c.ShouldBindJSON(&body); err != nil {
			c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
			return
		}
	}
	result, err := h.providerLeadSyncer.Sync(c.Request.Context(), strings.TrimSpace(body.OrganizationID), strings.TrimSpace(body.Owner))
	if err != nil {
		log.Printf("leads-core: manual provider-lead sync failed: %v", err)
		c.JSON(http.StatusBadGateway, errorPayload("sync_failed", "Provider lead sync failed against the actions gateway."))
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": result})
}

// DeleteProviderLeads is the org-scoped GDPR erasure path
// (DELETE /api/v1/provider-leads?organizationId=..., internal-key gated).
// It removes every provider lead the org holds — the containment guarantee
// for the person data provider_leads carries.
func (h *Handler) DeleteProviderLeads(c *gin.Context) {
	if h.providerLeadRepo == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("not_configured", "Provider lead storage is not configured."))
		return
	}
	orgID := strings.TrimSpace(c.Query("organizationId"))
	if orgID == "" {
		orgID = strings.TrimSpace(c.GetHeader("x-org-id"))
	}
	if orgID == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_org_id", "organizationId query parameter is required."))
		return
	}
	deleted, err := h.providerLeadRepo.DeleteByOrg(c.Request.Context(), orgID)
	if err != nil {
		log.Printf("leads-core: provider-lead erasure failed for org %s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, errorPayload("internal_error", "Internal error."))
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"organization_id": orgID, "deleted": deleted}})
}
