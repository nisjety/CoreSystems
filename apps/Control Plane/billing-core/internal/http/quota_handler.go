package http

import (
	"context"
	"errors"
	"log"
	"net/http"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/gin-gonic/gin"
)

// W2-CP-L1 (the Control half of U43 / WS6.D): the org-bound, read-only Quarry
// limits snapshot for the quarry-edge `billing_core` limits source.
//
//	GET /api/v1/billing/orgs/{orgId}/quarry/limits
//	X-Service-Id: quarry-edge
//	X-Service-Token: <QUARRY_BILLING_CORE_SERVICE_TOKEN>
//	X-Quarry-Org-Id: {orgId}
//
// Authorization is three checks, all required:
//  1. serviceAuthMiddleware: the static credential, scope billing:quota:read:any
//     (the route maps to that scope only; no :self delegation reaches it).
//  2. The principal is quarry-edge. Another machine that holds the same scope
//     for the generic quota route is refused here (403 principal_not_allowed).
//  3. Org binding: X-Quarry-Org-Id names the organization the edge is serving
//     and must equal the path organization (403 foreign_org otherwise). A
//     machine credential cannot be bound to one tenant, so the edge states the
//     tenant twice and a mismatch (a URL built from the wrong variable, a
//     confused-deputy request) is refused before anything is read. The answer
//     is scoped to the path organization and a snapshot for any other
//     organization is never served.
//
// 200 `{ "data": billing.QuarryLimits }` with `Cache-Control: private,
// max-age=30` (the edge's per-org cache window). Every error is `{ "error":
// { code, message } }` with `Cache-Control: no-store`; an unreadable store is
// 503 limits_unavailable so the edge applies its fail mode (floor by default).
const (
	quarryEdgeServicePrincipal = "quarry-edge"
	quarryLimitsScope          = "billing:quota:read:any"
	quarryLimitsOrgHeader      = "X-Quarry-Org-Id"
	quarryLimitsCacheControl   = "private, max-age=30"
	quarryLimitsErrorCache     = "no-store"
)

// quarryLimitsReader is the read the handler needs; *billing.Service satisfies
// it, and handler tests use a fake (no database).
type quarryLimitsReader interface {
	QuarryLimits(ctx context.Context, orgID string, now time.Time) (billing.QuarryLimits, error)
}

func (s *Server) registerQuarryLimitsRoute(group *gin.RouterGroup) {
	group.GET("/orgs/:orgId/quarry/limits", s.getQuarryLimits)
}

func quarryLimitsError(c *gin.Context, status int, code, message string) {
	c.Header("Cache-Control", quarryLimitsErrorCache)
	c.AbortWithStatusJSON(status, gin.H{"error": gin.H{"code": code, "message": message}})
}

func (s *Server) getQuarryLimits(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != quarryEdgeServicePrincipal ||
		c.GetString("service_scope") != quarryLimitsScope {
		quarryLimitsError(c, http.StatusForbidden, "principal_not_allowed", "This route serves the quarry-edge principal only.")
		return
	}
	orgID := c.Param("orgId")
	if !validPrincipal(orgID) {
		quarryLimitsError(c, http.StatusBadRequest, "invalid_org", "The organization id is invalid.")
		return
	}
	actingOrg, ok := singleTrimmedHeader(c.Request, quarryLimitsOrgHeader)
	if !ok {
		quarryLimitsError(c, http.StatusBadRequest, "org_binding_required", "Exactly one "+quarryLimitsOrgHeader+" header is required.")
		return
	}
	if !secureServiceValueEqual(orgID, actingOrg) {
		quarryLimitsError(c, http.StatusForbidden, "foreign_org", "The request is bound to a different organization.")
		return
	}
	if s.quarryLimits == nil {
		quarryLimitsError(c, http.StatusServiceUnavailable, "limits_unavailable", "Quarry limits are temporarily unavailable.")
		return
	}
	snapshot, err := s.quarryLimits.QuarryLimits(c.Request.Context(), orgID, time.Now().UTC())
	if err != nil {
		switch {
		case errors.Is(err, billing.ErrAllowanceInvalid):
			quarryLimitsError(c, http.StatusBadRequest, "invalid_org", "The organization id is invalid.")
		case errors.Is(err, billing.ErrOrganizationDeleted):
			quarryLimitsError(c, http.StatusForbidden, "organization_inactive", "This billing organization is inactive.")
		default:
			log.Printf("billing-core quarry limits read failed: %v", err)
			quarryLimitsError(c, http.StatusServiceUnavailable, "limits_unavailable", "Quarry limits are temporarily unavailable.")
		}
		return
	}
	if snapshot.OrgID != orgID {
		// Defence in depth: never serve another organization's answer.
		log.Printf("billing-core quarry limits snapshot organization mismatch; refused")
		quarryLimitsError(c, http.StatusInternalServerError, "limits_mismatch", "Quarry limits could not be served.")
		return
	}
	c.Header("Cache-Control", quarryLimitsCacheControl)
	c.JSON(http.StatusOK, gin.H{"data": snapshot})
}
