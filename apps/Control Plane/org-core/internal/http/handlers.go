package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

func (s *Server) health(c *gin.Context) {
	// Deep check: round-trip the DB so an unreachable/unauthenticated database
	// (e.g. a stale DB password) makes this container report unhealthy instead
	// of silently serving stale reads. Returning 503 fails the docker
	// `wget --spider /health` healthcheck.
	ctx, cancel := context.WithTimeout(c.Request.Context(), 2*time.Second)
	defer cancel()
	if err := s.orgService.Ping(ctx); err != nil {
		log.Printf("health: database ping failed: %v", err)
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"status":  "unhealthy",
			"service": "org-core",
			"error":   "database unreachable",
		})
		return
	}
	outboxStatus, err := s.orgService.GDPRAuditOutboxStatus(ctx)
	if err != nil {
		log.Printf("health: GDPR audit outbox status failed: %v", err)
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"status":  "unhealthy",
			"service": "org-core",
			"error":   "audit outbox unavailable",
		})
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"status":            "healthy",
		"service":           "org-core",
		"timestamp":         time.Now().UTC().Format(time.RFC3339),
		"gdpr_audit_outbox": outboxStatus,
	})
}

func (s *Server) getOrganization(c *gin.Context) {
	orgID := c.Param("id")
	orgData, err := s.orgService.GetOrganization(c.Request.Context(), orgID)
	if err != nil {
		if err == org.ErrNotFound {
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to get organization"})
		return
	}
	c.JSON(http.StatusOK, orgData)
}

func (s *Server) getUserOrganizations(c *gin.Context) {
	// Extract user ID from x-user-id header (set by auth proxy/frontend)
	userID := c.GetHeader("x-user-id")
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "user not authenticated"})
		return
	}

	// Get organizations where user is a member
	orgs, err := s.orgService.ListUserOrganizations(c.Request.Context(), userID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list organizations"})
		return
	}
	c.JSON(http.StatusOK, orgs)
}

func (s *Server) createOrganization(c *gin.Context) {
	var req struct {
		ID            string         `json:"id,omitempty"`
		Name          string         `json:"name" binding:"required"`
		Slug          string         `json:"slug,omitempty"`
		Plan          string         `json:"plan,omitempty"`
		OrgNumber     string         `json:"org_number,omitempty"`
		BrregData     map[string]any `json:"brreg_data,omitempty"`
		Metadata      map[string]any `json:"metadata,omitempty"`
		PrimaryDomain string         `json:"primary_domain,omitempty"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	// Extract user ID from header - the creator becomes the owner
	userID := c.GetHeader("x-user-id")
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "user not authenticated"})
		return
	}

	// Honour an externally-supplied id so an upstream identity provider (Better
	// Auth, which owns org membership) can mirror an org into org-core under the
	// SAME id. The upsert below is idempotent, so re-mirroring an existing id is
	// safe. Fall back to a generated id when none is provided (legacy callers).
	orgID := strings.TrimSpace(req.ID)
	if orgID == "" {
		orgID = generateOrgID()
	}

	newOrg := org.Organization{
		ID:            orgID,
		Name:          req.Name,
		Slug:          req.Slug,
		Plan:          "free",
		Status:        "active",
		Metadata:      req.Metadata,
		PrimaryDomain: strings.ToLower(strings.TrimSpace(req.PrimaryDomain)),
	}
	if req.OrgNumber != "" {
		newOrg.OrgNumber = &req.OrgNumber
		// Client-provided registry data is evidence for a later server-side
		// lookup, never proof of legal-entity verification.
		newOrg.VerificationStatus = "unverified"
		newOrg.BrregData = req.BrregData
	}

	if err := s.orgService.ProvisionOrganizationWithOwner(c.Request.Context(), newOrg, userID); err != nil {
		log.Printf("createOrganization: atomic provisioning failed org=%s user=%s: %v", newOrg.ID, userID, err)
		if errors.Is(err, org.ErrOwnerConflict) {
			c.JSON(http.StatusConflict, gin.H{"error": gin.H{
				"code":    "organization_owner_conflict",
				"message": "organization already has a different owner",
			}})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": gin.H{
			"code":    "organization_provisioning_failed",
			"message": "failed to provision organization and owner",
		}})
		return
	}

	// Re-fetch from DB so the response includes server-generated timestamps
	// (created_at / updated_at are set by DEFAULT NOW() — not present on the local struct).
	if fetchedOrg, fetchErr := s.orgService.GetOrganization(c.Request.Context(), newOrg.ID); fetchErr == nil {
		c.JSON(http.StatusCreated, fetchedOrg)
		return
	}
	c.JSON(http.StatusCreated, newOrg)
}

func generateOrgID() string {
	return "org_" + fmt.Sprintf("%d", time.Now().UnixNano()/1000000)
}

// searchBrreg proxies a name-based search to the Norwegian Enhetsregisteret.
// GET /api/v1/brreg/search?q=<name>&size=<n>
func (s *Server) searchBrreg(c *gin.Context) {
	q := strings.TrimSpace(c.Query("q"))
	if q == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "query parameter 'q' is required"})
		return
	}
	size := 10
	if sizeStr := c.Query("size"); sizeStr != "" {
		if n, err := strconv.Atoi(sizeStr); err == nil && n > 0 && n <= 50 {
			size = n
		}
	}

	results, err := s.brregClient.SearchByName(c.Request.Context(), q, size)
	if err != nil {
		c.JSON(http.StatusBadGateway, gin.H{"error": "failed to query Enhetsregisteret"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"results": results, "count": len(results)})
}

// lookupBrreg proxies a single org lookup by org number.
// GET /api/v1/brreg/:orgnr
func (s *Server) lookupBrreg(c *gin.Context) {
	orgnr := c.Param("orgnr")
	enhet, err := s.brregClient.LookupByOrgNr(c.Request.Context(), orgnr)
	if err != nil {
		c.JSON(http.StatusBadGateway, gin.H{"error": "failed to query Enhetsregisteret"})
		return
	}
	if enhet == nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "organization not found in Enhetsregisteret"})
		return
	}
	c.JSON(http.StatusOK, enhet)
}

// verifyOrgBrreg attaches Brreg verification data to an existing organization.
// PATCH /api/v1/organizations/:id/brreg
// Body: { "org_number": "...", "brreg_data": {...}, "verification_status": "verified"|"unverified" }
func (s *Server) verifyOrgBrreg(c *gin.Context) {
	orgID := c.Param("id")
	var req struct {
		OrgNumber          string         `json:"org_number" binding:"required"`
		BrregData          map[string]any `json:"brreg_data,omitempty"`
		VerificationStatus string         `json:"verification_status,omitempty"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "org_number is required"})
		return
	}
	verificationStatus := req.VerificationStatus
	if verificationStatus == "" {
		if req.BrregData != nil {
			verificationStatus = "verified"
		} else {
			verificationStatus = "unverified"
		}
	}
	if err := s.orgService.UpdateBrregVerification(c.Request.Context(), orgID, req.OrgNumber, req.BrregData, verificationStatus); err != nil {
		if err == org.ErrNotFound {
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to update brreg verification"})
		return
	}

	// Return the updated org
	orgData, err := s.orgService.GetOrganization(c.Request.Context(), orgID)
	if err != nil {
		c.JSON(http.StatusOK, gin.H{"message": "brreg verification updated"})
		return
	}
	c.JSON(http.StatusOK, orgData)
}

func (s *Server) updateCapabilities(c *gin.Context) {
	orgID := c.Param("id")
	var req struct {
		Capabilities map[string]bool `json:"capabilities" binding:"required"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body, capabilities required"})
		return
	}

	// Get the organization to ensure it exists
	orgData, err := s.orgService.GetOrganization(c.Request.Context(), orgID)
	if err != nil {
		if err == org.ErrNotFound {
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to get organization"})
		return
	}

	// Update capabilities in metadata
	if orgData.Metadata == nil {
		orgData.Metadata = make(map[string]interface{})
	}
	orgData.Metadata["capabilities"] = req.Capabilities

	// For demo purposes, mark provisioning as complete immediately
	// In production, this would be set by a background provisioning service
	orgData.Metadata["provisioningStatus"] = "complete"

	// Store updated organization
	if err := s.orgService.UpsertFromAuthEvent(c.Request.Context(), orgData.ID, orgData.Name, orgData.Slug, orgData.Metadata); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to update capabilities"})
		return
	}

	// Return the updated organization
	c.JSON(http.StatusOK, orgData)
}

func (s *Server) updatePlan(c *gin.Context) {
	orgID := c.Param("id")
	var req struct {
		Plan   string `json:"plan" binding:"required"`
		Reason string `json:"reason,omitempty"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "plan is required"})
		return
	}

	changedBy := c.GetHeader("x-user-id")
	orgData, err := s.orgService.UpdatePlan(c.Request.Context(), orgID, req.Plan, changedBy, req.Reason)
	if err != nil {
		switch {
		case err == org.ErrNotFound:
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
		case strings.Contains(err.Error(), "invalid plan"):
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid plan"})
		default:
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to update plan"})
		}
		return
	}

	c.JSON(http.StatusOK, orgData)
}

func (s *Server) getEntitlements(c *gin.Context) {
	orgID := c.Param("id")
	entitlements, err := s.orgService.GetEntitlements(c.Request.Context(), orgID)
	if err != nil {
		if err == org.ErrNotFound {
			c.JSON(http.StatusNotFound, gin.H{"error": "entitlements not found"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to get entitlements"})
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"organization_id": orgID,
		"entitlements":    entitlements,
	})
}

// listMembers returns all active members of an organization.
// GET /orgs/:id/members
func (s *Server) listMembers(c *gin.Context) {
	orgID := c.Param("id")
	members, err := s.orgService.ListOrganizationMembers(c.Request.Context(), orgID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list members"})
		return
	}
	if members == nil {
		members = []org.OrgMember{}
	}
	c.JSON(http.StatusOK, gin.H{"members": members, "count": len(members)})
}

// inviteMember adds or records an invitation for a user to join an organization.
// POST /orgs/:id/members/invite
// Body: { "email": "...", "role": "admin"|"member"|"viewer" }
func (s *Server) inviteMember(c *gin.Context) {
	orgID := c.Param("id")
	var req struct {
		Email string `json:"email" binding:"required"`
		Role  string `json:"role,omitempty"`
	}
	if err := c.ShouldBindJSON(&req); err != nil || req.Email == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "email is required"})
		return
	}
	role := req.Role
	if role == "" {
		role = "member"
	}

	// Validate role
	validRoles := map[string]bool{"owner": true, "admin": true, "member": true, "viewer": true}
	if !validRoles[role] {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid role; must be admin, member, or viewer"})
		return
	}

	invitingUserID := c.GetHeader("x-user-id")

	// Try to look up user by email in user-core.
	// If found, add them directly as a member.
	if s.userService != "" {
		lookupURL := strings.TrimRight(s.userService, "/") + "/api/v1/users/by-email/" + req.Email
		lookupReq, _ := http.NewRequestWithContext(c.Request.Context(), http.MethodGet, lookupURL, nil)
		s.applyUserServiceAuth(lookupReq)
		if resp, err := s.httpClient.Do(lookupReq); err == nil {
			defer resp.Body.Close()
			if resp.StatusCode == http.StatusOK {
				var userPayload struct {
					ID string `json:"id"`
				}
				if err := json.NewDecoder(resp.Body).Decode(&userPayload); err == nil && userPayload.ID != "" {
					if addErr := s.orgService.AddOrganizationMember(c.Request.Context(), orgID, userPayload.ID, role); addErr == nil {
						c.JSON(http.StatusOK, gin.H{
							"invitation_id": "inv_" + strings.ReplaceAll(req.Email, "@", "_at_"),
							"status":        "active",
							"message":       "user added as member",
							"email":         req.Email,
							"role":          role,
						})
						return
					}
				}
			}
		}
	}

	// User not found or lookup failed: record as pending invite in organization_members table
	invID, dbErr := s.orgService.AddPendingInvite(c.Request.Context(), orgID, req.Email, role, invitingUserID)
	if dbErr != nil {
		invID = fmt.Sprintf("inv_%d", time.Now().UnixNano()/1_000_000)
	}

	c.JSON(http.StatusAccepted, gin.H{
		"invitation_id": invID,
		"status":        "pending",
		"message":       "invitation recorded; user will be added when they register",
		"email":         req.Email,
		"role":          role,
	})
}

// removeMember soft-removes a member from an organization.
// DELETE /orgs/:id/members/:userId
func (s *Server) removeMember(c *gin.Context) {
	orgID := c.Param("id")
	userID := c.Param("userId")
	if err := s.orgService.RemoveOrganizationMember(c.Request.Context(), orgID, userID); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to remove member"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true})
}

// getOrganizationByTenant resolves an organization by provider + tenant id.
// GET /internal/orgs/by-tenant?provider=microsoft&tenantId=<tenant>
func (s *Server) getOrganizationByTenant(c *gin.Context) {
	provider := strings.TrimSpace(c.Query("provider"))
	tenantID := strings.TrimSpace(c.Query("tenantId"))
	if tenantID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "tenantId is required"})
		return
	}

	orgData, err := s.orgService.GetOrganizationByTenant(c.Request.Context(), provider, tenantID)
	if err != nil {
		if err == org.ErrNotFound {
			c.JSON(http.StatusNotFound, gin.H{"error": "organization not found"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve organization"})
		return
	}

	c.JSON(http.StatusOK, orgData)
}

// ensureOrganizationFromTenant resolves an organization by provider+tenant, creating one if missing.
// POST /internal/orgs/ensure-from-tenant
func (s *Server) ensureOrganizationFromTenant(c *gin.Context) {
	var req struct {
		Provider      string   `json:"provider"`
		TenantID      string   `json:"tenantId" binding:"required"`
		OwnerUserID   string   `json:"ownerUserId" binding:"required"`
		DisplayName   string   `json:"displayName"`
		PrimaryDomain string   `json:"primaryDomain"`
		Domains       []string `json:"domains"`
		Region        string   `json:"region"`
		DefaultLocale string   `json:"defaultLocale"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "tenantId is required"})
		return
	}

	orgData, created, err := s.orgService.EnsureOrganizationFromTenant(
		c.Request.Context(),
		req.Provider,
		req.TenantID,
		req.OwnerUserID,
		req.DisplayName,
		req.PrimaryDomain,
		req.Domains,
		req.Region,
		req.DefaultLocale,
	)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to ensure organization"})
		return
	}

	status := http.StatusOK
	if created {
		status = http.StatusCreated
	}
	c.JSON(status, gin.H{
		"organization": orgData,
		"created":      created,
	})
}

// updateOnboardingState updates/creates onboarding state for an organization.
// POST /internal/orgs/:orgId/onboarding/state
func (s *Server) updateOnboardingState(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("orgId"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "orgId is required"})
		return
	}

	var req struct {
		Status string         `json:"status"`
		Steps  map[string]any `json:"steps"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	if err := s.orgService.UpdateOnboardingState(c.Request.Context(), orgID, req.Status, req.Steps); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to update onboarding state"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"ok": true})
}

// reconcileOrganizationProjection applies a monotonic Auth Core organization
// snapshot. It is separate from the browser-facing create route so identity,
// owner, and revision arrive in one authenticated machine contract.
func (s *Server) reconcileOrganizationProjection(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("orgId"))
	var req struct {
		Name        string         `json:"name" binding:"required"`
		Slug        string         `json:"slug"`
		Metadata    map[string]any `json:"metadata"`
		OwnerUserID string         `json:"ownerUserId" binding:"required"`
		Revision    int64          `json:"revision" binding:"required"`
	}
	if orgID == "" || c.ShouldBindJSON(&req) != nil || strings.TrimSpace(req.Name) == "" || strings.TrimSpace(req.OwnerUserID) == "" || req.Revision < 1 {
		c.JSON(http.StatusBadRequest, gin.H{"error": "orgId, name, ownerUserId, and positive revision are required"})
		return
	}

	applied, err := s.orgService.ReconcileOrganizationProjection(
		c.Request.Context(),
		org.Organization{
			ID:       orgID,
			Name:     strings.TrimSpace(req.Name),
			Slug:     strings.TrimSpace(req.Slug),
			Plan:     "free",
			Status:   "active",
			Metadata: req.Metadata,
		},
		strings.TrimSpace(req.OwnerUserID),
		req.Revision,
	)
	if err != nil {
		if errors.Is(err, org.ErrOrganizationDeleted) ||
			errors.Is(err, org.ErrOwnerConflict) ||
			errors.Is(err, org.ErrProjectionConflict) {
			c.JSON(http.StatusConflict, gin.H{"error": "organization projection conflicts with local lifecycle state"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to reconcile organization"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ok": true, "applied": applied})
}

// reconcileOrganizationMember is the idempotent Auth Core projection endpoint.
// It is internal-key protected and never trusts browser-supplied identity.
func (s *Server) reconcileOrganizationMember(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("orgId"))
	var req struct {
		UserID   string `json:"userId" binding:"required"`
		Role     string `json:"role"`
		Action   string `json:"action" binding:"required"`
		Revision int64  `json:"revision" binding:"required"`
	}
	if orgID == "" || c.ShouldBindJSON(&req) != nil || strings.TrimSpace(req.UserID) == "" || req.Revision < 1 {
		c.JSON(http.StatusBadRequest, gin.H{"error": "orgId, userId, action, and positive revision are required"})
		return
	}

	action := strings.ToLower(strings.TrimSpace(req.Action))
	if action != "upsert" && action != "remove" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "action must be upsert or remove"})
		return
	}
	role := strings.ToLower(strings.TrimSpace(req.Role))
	if action == "upsert" && role != "owner" && role != "admin" && role != "member" && role != "viewer" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "role must be owner, admin, member, or viewer"})
		return
	}
	applied, err := s.orgService.ReconcileOrganizationMember(c.Request.Context(), orgID, req.UserID, role, action, req.Revision)
	if err != nil {
		c.JSON(http.StatusConflict, gin.H{"error": "failed to reconcile member"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"ok": true, "applied": applied})
}

// reconcileOrganizationDeletion is the idempotent Auth Core projection
// endpoint. Authentication is enforced by the internal API-key middleware.
func (s *Server) reconcileOrganizationDeletion(c *gin.Context) {
	orgID := strings.TrimSpace(c.Param("orgId"))
	var req struct {
		Revision int64 `json:"revision" binding:"required"`
	}
	if orgID == "" || c.ShouldBindJSON(&req) != nil ||
		req.Revision < 1 || req.Revision > org.MaxSafeAuthRevision {
		c.JSON(http.StatusBadRequest, gin.H{"error": "orgId and a positive safe revision are required"})
		return
	}

	receipt, applied, err := s.orgService.ReconcileOrganizationDeletion(
		c.Request.Context(), orgID, req.Revision,
	)
	if err != nil {
		if errors.Is(err, org.ErrProjectionConflict) ||
			errors.Is(err, org.ErrOrganizationDeleted) {
			c.JSON(http.StatusConflict, gin.H{"error": "organization deletion conflicts with local lifecycle state"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to reconcile organization deletion"})
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"ok": true, "applied": applied, "receipt": json.RawMessage(receipt),
	})
}

type loginRequest struct {
	Email    string `json:"email"`
	Password string `json:"password"`
}

func (s *Server) login(c *gin.Context) {
	var req loginRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid login payload"})
		return
	}

	body, _ := json.Marshal(req)
	paths := []string{"/api/auth/sign-in/email", "/api/auth/sign-in"}

	for _, path := range paths {
		upstreamReq, err := http.NewRequestWithContext(c.Request.Context(), http.MethodPost, strings.TrimRight(s.authService, "/")+path, bytes.NewReader(body))
		if err != nil {
			log.Printf("failed to build auth login request: %v", err)
			continue
		}
		upstreamReq.Header.Set("Content-Type", "application/json")
		if authz := c.GetHeader("Authorization"); authz != "" {
			upstreamReq.Header.Set("Authorization", authz)
		}
		if cookie := c.GetHeader("Cookie"); cookie != "" {
			upstreamReq.Header.Set("Cookie", cookie)
		}

		resp, err := s.httpClient.Do(upstreamReq)
		if err != nil {
			continue
		}

		if resp.StatusCode == http.StatusNotFound {
			resp.Body.Close()
			continue
		}

		for _, setCookie := range resp.Header.Values("Set-Cookie") {
			c.Writer.Header().Add("Set-Cookie", setCookie)
		}
		payload, readErr := io.ReadAll(resp.Body)
		resp.Body.Close()
		if readErr != nil {
			c.JSON(http.StatusBadGateway, gin.H{"error": "failed to read auth service response"})
			return
		}
		c.Data(resp.StatusCode, "application/json", payload)
		return
	}

	c.JSON(http.StatusBadGateway, gin.H{"error": "auth service unavailable"})
}

func (s *Server) getCurrentUser(c *gin.Context) {
	sessionReq, err := http.NewRequestWithContext(c.Request.Context(), http.MethodGet, strings.TrimRight(s.authService, "/")+"/api/auth/get-session", nil)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to build auth service request"})
		return
	}
	if authz := c.GetHeader("Authorization"); authz != "" {
		sessionReq.Header.Set("Authorization", authz)
	}
	if cookie := c.GetHeader("Cookie"); cookie != "" {
		sessionReq.Header.Set("Cookie", cookie)
	}

	sessionResp, err := s.httpClient.Do(sessionReq)
	if err != nil {
		c.JSON(http.StatusBadGateway, gin.H{"error": "auth service unavailable"})
		return
	}
	defer sessionResp.Body.Close()

	if sessionResp.StatusCode >= 400 {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
		return
	}

	var session map[string]any
	if err := json.NewDecoder(sessionResp.Body).Decode(&session); err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid auth session response"})
		return
	}

	userID := extractUserID(session)
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "user id not found in session"})
		return
	}

	userReq, _ := http.NewRequestWithContext(c.Request.Context(), http.MethodGet, strings.TrimRight(s.userService, "/")+"/api/v1/users/"+userID, nil)
	s.applyUserServiceAuth(userReq)
	userResp, err := s.httpClient.Do(userReq)
	if err != nil {
		c.JSON(http.StatusBadGateway, gin.H{"error": "user service unavailable"})
		return
	}
	defer userResp.Body.Close()

	payload, _ := io.ReadAll(userResp.Body)
	c.Data(userResp.StatusCode, "application/json", payload)
}

func extractUserID(session map[string]any) string {
	if user, ok := session["user"].(map[string]any); ok {
		if id, ok := user["id"].(string); ok {
			return id
		}
	}
	if data, ok := session["data"].(map[string]any); ok {
		if user, ok := data["user"].(map[string]any); ok {
			if id, ok := user["id"].(string); ok {
				return id
			}
		}
	}
	return ""
}

// memberUserInfo is the minimal user profile fetched from user-core for member enrichment.
type memberUserInfo struct {
	DisplayName string `json:"display_name"`
	Email       string `json:"email"`
	AvatarURL   string `json:"avatar_url"`
}

// fetchUserProfile retrieves a user's display info from user-core by user ID.
func (s *Server) fetchUserProfile(ctx context.Context, userID string) (*memberUserInfo, error) {
	url := strings.TrimRight(s.userService, "/") + "/api/v1/users/" + userID
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	s.applyUserServiceAuth(req)
	resp, err := s.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("user-core: status %d", resp.StatusCode)
	}
	var u memberUserInfo
	if err := json.NewDecoder(resp.Body).Decode(&u); err != nil {
		return nil, err
	}
	return &u, nil
}

func (s *Server) applyUserServiceAuth(req *http.Request) {
	if req == nil || s.userServiceToken == "" {
		return
	}
	req.Header.Set("X-Service-Token", s.userServiceToken)
	req.Header.Set("X-Service-Id", "org-core")
}

// searchMembers returns member suggestions for @mention autocomplete.
// GET /orgs/:id/members/search?q=<query>&limit=<n>
//
// Active members are enriched concurrently via user-core (name + email + avatar).
// Invited members are matched directly against the stored invited_email — no remote call.
// Results are sorted: active members first, then alphabetically by display name.
func (s *Server) searchMembers(c *gin.Context) {
	orgID := c.Param("id")
	query := strings.ToLower(strings.TrimSpace(c.Query("q")))
	if query == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "query parameter 'q' is required"})
		return
	}
	limit := 10
	if ls := c.Query("limit"); ls != "" {
		if n, err := strconv.Atoi(ls); err == nil && n >= 1 && n <= 20 {
			limit = n
		}
	}

	members, err := s.orgService.ListOrganizationMembers(c.Request.Context(), orgID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to search members"})
		return
	}

	var (
		mu          sync.Mutex
		suggestions []org.MemberSuggestion
		wg          sync.WaitGroup
	)

	// Cap concurrent user-core fetches to avoid overloading the service on large orgs.
	const maxActiveFetch = 50
	activeCount := 0

	for _, m := range members {
		switch m.Status {
		case "invited":
			// Invited members: email is stored locally — match directly, no remote call.
			if m.InvitedEmail == "" {
				continue
			}
			if !strings.Contains(strings.ToLower(m.InvitedEmail), query) {
				continue
			}
			mu.Lock()
			suggestions = append(suggestions, org.MemberSuggestion{
				UserID:      m.ID,
				DisplayName: m.InvitedEmail,
				Email:       m.InvitedEmail,
				Role:        m.Role,
				Status:      "invited",
			})
			mu.Unlock()

		case "active":
			if s.userService == "" || activeCount >= maxActiveFetch {
				continue
			}
			activeCount++
			m := m
			wg.Add(1)
			go func() {
				defer wg.Done()
				u, err := s.fetchUserProfile(c.Request.Context(), m.UserID)
				if err != nil {
					return // graceful degradation: skip members we can't enrich
				}
				nameMatch := strings.Contains(strings.ToLower(u.DisplayName), query)
				emailMatch := strings.Contains(strings.ToLower(u.Email), query)
				if !nameMatch && !emailMatch {
					return
				}
				mu.Lock()
				suggestions = append(suggestions, org.MemberSuggestion{
					UserID:      m.UserID,
					DisplayName: u.DisplayName,
					Email:       u.Email,
					AvatarURL:   u.AvatarURL,
					Role:        m.Role,
					Status:      "active",
				})
				mu.Unlock()
			}()
		}
	}
	wg.Wait()

	// Sort: active first, then alphabetically within each group.
	sort.Slice(suggestions, func(i, j int) bool {
		if suggestions[i].Status != suggestions[j].Status {
			return suggestions[i].Status == "active"
		}
		return strings.ToLower(suggestions[i].DisplayName) < strings.ToLower(suggestions[j].DisplayName)
	})

	if len(suggestions) > limit {
		suggestions = suggestions[:limit]
	}
	if suggestions == nil {
		suggestions = []org.MemberSuggestion{}
	}

	c.JSON(http.StatusOK, gin.H{
		"results": suggestions,
		"query":   query,
		"count":   len(suggestions),
	})
}
