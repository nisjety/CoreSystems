package http

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/browsersites"
	"github.com/gin-gonic/gin"
)

// browserSiteService is the seam the browser site routes depend on.
// *browsersites.Service satisfies it; the handler tests use the real service over
// an in-memory store, so the real router and service-auth middleware run without a
// database.
type browserSiteService interface {
	Get(ctx context.Context, orgID string) (*browsersites.PolicySet, error)
	Put(ctx context.Context, orgID, actorID string, req browsersites.PutRequest) (*browsersites.PolicySet, error)
	Prefs(ctx context.Context, orgID, userID string) (browsersites.PrefSet, error)
	SetPref(ctx context.Context, orgID, userID, host string, req browsersites.PrefRequest) (browsersites.Pref, error)
	DeletePref(ctx context.Context, orgID, userID, host string) error
	Explain(ctx context.Context, orgID, userID, host string) (browsersites.Explanation, error)
	Resolve(ctx context.Context, orgID, userID string, hosts []string) (browsersites.Resolution, error)
}

// SetBrowserSites wires the site policy service (WS21-S3). A server without it
// answers the routes 503 instead of serving a policy it cannot read.
func (s *Server) SetBrowserSites(service browserSiteService) {
	s.browserSites = service
}

// browserSiteTimeout bounds one policy read or write, as the model policy does.
const browserSiteTimeout = 5 * time.Second

// browserSiteCodeLimit is the code a member sees when the choice list is full.
const browserSiteCodeLimit = "pref_limit_reached"

// The membership and role rules are the model policy's (WS10-S2): the caller's
// role comes from org-core's membership table, never from the forwarded header.
// authorizeModelPolicyAdmin and authorizeModelPolicyReader are reused as they are.

// getBrowserSites serves GET /api/v1/organizations/:id/browser-sites: the admin's
// policy, for the settings view. Owner or admin only.
func (s *Server) getBrowserSites(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	if _, ok := s.authorizeModelPolicyAdmin(c, orgID); !ok {
		return
	}
	if s.browserSites == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	set, err := s.browserSites.Get(ctx, orgID)
	if err != nil {
		log.Printf("browser sites: read failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read browser site policy"})
		return
	}
	c.JSON(http.StatusOK, set)
}

// putBrowserSites serves PUT /api/v1/organizations/:id/browser-sites. The body
// replaces the whole policy and must carry the revision it was edited from: a stale
// editor gets 409 with the current revision, never a silent overwrite.
func (s *Server) putBrowserSites(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	actorID, ok := s.authorizeModelPolicyAdmin(c, orgID)
	if !ok {
		return
	}
	if s.browserSites == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	var req browsersites.PutRequest
	if !decodeBrowserSiteBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	saved, err := s.browserSites.Put(ctx, orgID, actorID, req)
	if err != nil {
		var invalid *browsersites.ValidationError
		var conflict *browsersites.RevisionConflictError
		switch {
		case errors.As(err, &invalid):
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid browser site policy", "code": "invalid_browser_site_policy", "details": invalid.Errors})
		case errors.As(err, &conflict):
			c.JSON(http.StatusConflict, gin.H{"error": "the browser site policy changed since it was read", "code": "revision_conflict", "current_revision": conflict.Current})
		case errors.Is(err, browsersites.ErrOrganizationNotFound):
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
		default:
			log.Printf("browser sites: write failed for org=%s: %v", orgID, err)
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to write browser site policy"})
		}
		return
	}
	log.Printf("browser sites: org=%s revision=%d written by actor=%s", orgID, saved.Revision, actorID)
	c.JSON(http.StatusOK, saved)
}

// explainBrowserSite serves GET /api/v1/organizations/:id/browser-sites/explain?host=.
// It answers for the caller alone: the admin's rule and the caller's own choice.
func (s *Server) explainBrowserSite(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, _, ok := s.authorizeModelPolicyReader(c, orgID)
	if !ok {
		return
	}
	if s.browserSites == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	explanation, err := s.browserSites.Explain(ctx, orgID, callerID, c.Query("host"))
	if err != nil {
		if writeBrowserSiteValidation(c, err) {
			return
		}
		log.Printf("browser sites: explain failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to explain browser site policy"})
		return
	}
	c.JSON(http.StatusOK, explanation)
}

// listBrowserSitePrefs serves GET /api/v1/organizations/:id/browser-sites/me: the
// caller's own choices and the policy revision they were read against.
func (s *Server) listBrowserSitePrefs(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, _, ok := s.authorizeModelPolicyReader(c, orgID)
	if !ok {
		return
	}
	if s.browserSites == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	prefs, err := s.browserSites.Prefs(ctx, orgID, callerID)
	if err != nil {
		log.Printf("browser sites: choices read failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read browser site choices"})
		return
	}
	c.JSON(http.StatusOK, prefs)
}

// putBrowserSitePref serves PUT /api/v1/organizations/:id/browser-sites/me/:host.
// The caller records their own choice for one host. The choice must name the
// policy revision the caller read: a choice made against a superseded policy is
// refused with 409 so the caller decides again on the current one.
func (s *Server) putBrowserSitePref(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, _, ok := s.authorizeModelPolicyReader(c, orgID)
	if !ok {
		return
	}
	if s.browserSites == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	var req browsersites.PrefRequest
	if !decodeBrowserSiteBody(c, &req) {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	saved, err := s.browserSites.SetPref(ctx, orgID, callerID, c.Param("host"), req)
	if err != nil {
		if writeBrowserSiteValidation(c, err) {
			return
		}
		var conflict *browsersites.RevisionConflictError
		switch {
		case errors.As(err, &conflict):
			c.JSON(http.StatusConflict, gin.H{"error": "the browser site policy changed since it was read", "code": "revision_conflict", "current_revision": conflict.Current})
		case errors.Is(err, browsersites.ErrPrefLimitReached):
			c.JSON(http.StatusUnprocessableEntity, gin.H{"error": "too many remembered sites", "code": browserSiteCodeLimit})
		case errors.Is(err, browsersites.ErrOrganizationNotFound):
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
		default:
			log.Printf("browser sites: choice write failed for org=%s: %v", orgID, err)
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to write browser site choice"})
		}
		return
	}
	c.JSON(http.StatusOK, saved)
}

// deleteBrowserSitePref serves DELETE /api/v1/organizations/:id/browser-sites/me/:host.
// It removes the caller's own choice for one host, so the next use asks again.
func (s *Server) deleteBrowserSitePref(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	callerID, _, ok := s.authorizeModelPolicyReader(c, orgID)
	if !ok {
		return
	}
	if s.browserSites == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	err := s.browserSites.DeletePref(ctx, orgID, callerID, c.Param("host"))
	if err == nil {
		c.Status(http.StatusNoContent)
		return
	}
	if writeBrowserSiteValidation(c, err) {
		return
	}
	if errors.Is(err, browsersites.ErrPrefNotFound) {
		c.JSON(http.StatusNotFound, gin.H{"error": "no remembered choice for this site"})
		return
	}
	log.Printf("browser sites: choice delete failed for org=%s: %v", orgID, err)
	c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to delete browser site choice"})
}

// getBrowserSitesInternal serves GET /internal/orgs/:orgId/browser-sites/resolve?host=&user_id=,
// the browser broker's pull (WS21-S3). Access requires org:browser-site:read:any,
// which only the browser-broker principal holds. The answer is computed on every
// call and marked no-store. user_id is optional: absent, the admin's answer applies
// alone. Present but blank is refused rather than read as absent, so a caller bug
// cannot widen a member's access. A 5xx is "unknown" to the broker, which fails
// closed.
func (s *Server) getBrowserSitesInternal(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "organization id is required"})
		return
	}
	userID := ""
	if requested, present := c.GetQuery("user_id"); present {
		userID = strings.TrimSpace(requested)
		if userID == "" || len(userID) > maxModelEntitlementUserIDLength {
			c.JSON(http.StatusBadRequest, gin.H{"error": "user_id is invalid"})
			return
		}
	}
	if s.browserSites == nil {
		log.Printf("browser sites: no service configured for org=%s", orgID)
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "browser site policy unavailable"})
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), browserSiteTimeout)
	defer cancel()
	resolution, err := s.browserSites.Resolve(ctx, orgID, userID, c.QueryArray("host"))
	if err != nil {
		if writeBrowserSiteValidation(c, err) {
			return
		}
		log.Printf("browser sites: resolve failed for org=%s: %v", orgID, err)
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve browser site policy"})
		return
	}
	c.JSON(http.StatusOK, resolution)
}

// writeBrowserSiteValidation answers a validation failure with 400 and the field
// errors, and reports whether it did.
func writeBrowserSiteValidation(c *gin.Context, err error) bool {
	var invalid *browsersites.ValidationError
	if !errors.As(err, &invalid) {
		return false
	}
	c.JSON(http.StatusBadRequest, gin.H{"error": "invalid browser site request", "code": "invalid_browser_site_policy", "details": invalid.Errors})
	return true
}

// decodeBrowserSiteBody reads one strict JSON body under the size limit. It writes
// the refusal itself and reports false when the request must stop.
func decodeBrowserSiteBody(c *gin.Context, into any) bool {
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, browsersites.MaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(into)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			c.JSON(http.StatusRequestEntityTooLarge, gin.H{"error": "request body too large", "code": "body_too_large"})
			return false
		}
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body", "code": "invalid_body"})
		return false
	}
	return true
}
