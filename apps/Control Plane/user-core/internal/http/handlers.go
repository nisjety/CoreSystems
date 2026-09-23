package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

type authCoreTokenResponse struct {
	Found     bool   `json:"found"`
	TokenRef  string `json:"tokenRef"`
	Scope     string `json:"scope"`
	ExpiresAt string `json:"expires_at"`
	Error     string `json:"error"`
}

type authMembershipDecision struct {
	Version string  `json:"version"`
	Member  bool    `json:"member"`
	Role    *string `json:"role"`
}

var ErrMembershipNotFound = errors.New("authoritative organization membership not found")

func verifiedProfileFromContext(c *gin.Context) (email, name, avatar string) {
	return c.GetString("user_email"), c.GetString("user_name"), c.GetString("user_avatar")
}

func (s *Server) ensureCanonicalCurrentUser(c *gin.Context) (*users.User, string, bool) {
	userID, ok := getUserIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return nil, "", false
	}

	email, name, avatar := verifiedProfileFromContext(c)
	user, err := s.userService.GetOrCreateUser(
		c.Request.Context(),
		userID,
		email,
		name,
		avatar,
	)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to resolve current user")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to resolve current user"})
		return nil, "", false
	}

	return user, userID, true
}

// resolveMembershipFromAuthCore asks the Better Auth database authority for
// one exact (user, organization) membership. Conversation access deliberately
// has no primary/latest-organization fallback: callers must carry the active
// organization chosen by the validated session.
func (s *Server) resolveMembershipFromAuthCore(
	ctx context.Context,
	userID string,
	requestedOrgID string,
) (string, string, error) {
	userID = strings.TrimSpace(userID)
	requestedOrgID = strings.TrimSpace(requestedOrgID)
	if userID == "" || requestedOrgID == "" {
		return "", "", ErrMembershipNotFound
	}
	if strings.TrimSpace(s.authMembershipService) == "" || strings.TrimSpace(s.authMembershipToken) == "" {
		return "", "", fmt.Errorf("canonical membership authority is not configured")
	}

	body, err := json.Marshal(map[string]string{
		"userId": userID,
		"orgId":  requestedOrgID,
	})
	if err != nil {
		return "", "", fmt.Errorf("encode canonical membership request: %w", err)
	}
	response, err := s.callCanonicalMembershipAuthority(ctx, body)
	if err != nil {
		return "", "", err
	}
	defer func() {
		// The decision payload is already decoded (or the status rejected);
		// a close error on an idle response body is not actionable.
		_ = response.Body.Close()
	}()
	if response.StatusCode != http.StatusOK {
		return "", "", fmt.Errorf("canonical membership authority returned %d", response.StatusCode)
	}

	var decision authMembershipDecision
	decoder := json.NewDecoder(io.LimitReader(response.Body, 64<<10))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&decision); err != nil {
		return "", "", fmt.Errorf("decode canonical membership decision: %w", err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		return "", "", fmt.Errorf("decode canonical membership decision: trailing data")
	}
	if decision.Version != "v1" {
		return "", "", fmt.Errorf("canonical membership authority returned unsupported contract version")
	}
	if !decision.Member {
		if decision.Role != nil {
			return "", "", fmt.Errorf("canonical membership denial included a role")
		}
		return "", "", ErrMembershipNotFound
	}
	if decision.Role == nil {
		return "", "", fmt.Errorf("canonical membership grant omitted role")
	}
	role := strings.ToLower(strings.TrimSpace(*decision.Role))
	if role != "owner" && role != "admin" && role != "member" && role != "viewer" {
		return "", "", fmt.Errorf("canonical membership authority returned unsupported role")
	}
	return requestedOrgID, role, nil
}

// callCanonicalMembershipAuthority retries one transport-level failure only.
// The request is an idempotent read of an exact user/org pair; a retry avoids
// converting a stale keep-alive socket reset into a visible 503, while a
// non-200 decision remains fail-closed and is never retried.
func (s *Server) callCanonicalMembershipAuthority(ctx context.Context, body []byte) (*http.Response, error) {
	const attempts = 2
	const retryDelay = 50 * time.Millisecond
	url := s.authMembershipService + "/api/v1/internal/membership/decision"

	for attempt := range attempts {
		request, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
		if err != nil {
			return nil, fmt.Errorf("build canonical membership request: %w", err)
		}
		request.Header.Set("Content-Type", "application/json")
		request.Header.Set("X-User-Core-Membership-Token", s.authMembershipToken)

		response, err := s.httpClient.Do(request)
		if err == nil {
			return response, nil
		}
		if attempt == attempts-1 {
			return nil, fmt.Errorf("call canonical membership authority: %w", err)
		}

		timer := time.NewTimer(retryDelay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil, ctx.Err()
		case <-timer.C:
		}
	}

	return nil, fmt.Errorf("call canonical membership authority: retry attempts exhausted")
}

func (s *Server) handleMembershipResolutionError(
	c *gin.Context,
	userID string,
	requestedOrgID string,
	resolveErr error,
) (denied bool, continueRequest bool) {
	if !errors.Is(resolveErr, ErrMembershipNotFound) {
		log.Warn().Err(resolveErr).Str("user_id", strings.TrimSpace(userID)).Msg("auth-core membership authority unavailable for session context")
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "organization membership authority unavailable"})
		return false, false
	}

	userID = strings.TrimSpace(userID)
	requestedOrgID = strings.TrimSpace(requestedOrgID)
	if requestedOrgID == "" {
		// No exact organization was requested, so there is no scoped local row to
		// revoke. The caller still receives no organization or role grant.
		return true, true
	}
	if userID == "" || s.removeMembershipProjection == nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to revoke stale organization membership"})
		return true, false
	}
	if err := s.removeMembershipProjection(c.Request.Context(), userID, requestedOrgID); err != nil {
		log.Error().Err(err).Str("user_id", userID).Str("org_id", requestedOrgID).Msg("failed to revoke stale user-core membership projection")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to revoke stale organization membership"})
		return true, false
	}
	return true, true
}

// ============================================
// USER PROFILE ENDPOINTS
// ============================================

// getCurrentUserProfile retrieves the current authenticated user's profile
// GET /api/v1/users/me
func (s *Server) getCurrentUserProfile(c *gin.Context) {
	// Extract user ID from context (set by auth middleware)
	userIDStr, ok := requireContextUserID(c)
	if !ok {
		return
	}

	// Auto-provisioning attributes come only from Auth Core's verified session
	// response. Caller-supplied X-User-* headers are never identity evidence.
	email := c.GetString("user_email")
	name := c.GetString("user_name")
	avatar := c.GetString("user_avatar")

	// Get or create user (auto-provision from OAuth/Better Auth if needed)
	user, err := s.userService.GetOrCreateUser(c.Request.Context(), userIDStr, email, name, avatar)
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to get or create user")
		c.JSON(http.StatusInternalServerError, gin.H{
			"error": "Failed to retrieve user profile",
		})
		return
	}

	profile, profileErr := s.userService.GetUserProfile(c.Request.Context(), userIDStr)
	if profileErr != nil {
		profile = nil
	}

	position := ""
	department := ""
	presenceStatus := mapUserStatusToPresence(user.Status)
	firstName := ""
	lastName := ""
	phone := ""
	location := ""
	timezone := ""

	if profile != nil {
		phone = profile.Phone
		location = profile.Location
		timezone = profile.Timezone
		if profile.Metadata != nil {
			if v, ok := profile.Metadata["position"].(string); ok {
				position = v
			}
			if v, ok := profile.Metadata["department"].(string); ok {
				department = v
			}
			if v, ok := profile.Metadata["status"].(string); ok && v != "" {
				presenceStatus = v
			}
			if v, ok := profile.Metadata["firstName"].(string); ok {
				firstName = v
			}
			if v, ok := profile.Metadata["lastName"].(string); ok {
				lastName = v
			}
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"user": map[string]any{
			"id":                  user.ID,
			"email":               user.Email,
			"name":                user.Name,
			"display_name":        user.Name,
			"avatar":              user.Avatar,
			"email_verified":      user.EmailVerified,
			"onboarding_complete": user.OnboardingComplete,
			"status":              presenceStatus,
			"account_status":      user.Status,
			"position":            position,
			"department":          department,
			"first_name":          firstName,
			"last_name":           lastName,
			"phone":               phone,
			"location":            location,
			"timezone":            timezone,
			"created_at":          user.CreatedAt,
			"updated_at":          user.UpdatedAt,
			"last_login_at":       user.LastLoginAt,
		},
	})
}

// getSessionContext returns user/org/role/onboarding status for post-login routing.
// GET /api/v1/me/session-context
func (s *Server) getSessionContext(c *gin.Context) {
	user, userID, ok := s.ensureCanonicalCurrentUser(c)
	if !ok {
		return
	}

	// The gateway forwards the session's active organization via X-Org-Id so the
	// role/onboarding status reflect the org the user is currently acting as,
	// rather than always the primary membership.
	requestedOrg := strings.TrimSpace(c.GetHeader("X-Org-Id"))

	email, name, avatar := verifiedProfileFromContext(c)
	ctxData, err := s.userService.GetSessionContext(
		c.Request.Context(),
		userID,
		email,
		name,
		avatar,
		requestedOrg,
	)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to build session context")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to build session context"})
		return
	}

	orgID, role, resolveErr := s.resolveMembershipFromAuthCore(c.Request.Context(), userID, requestedOrg)
	if resolveErr != nil {
		denied, continueRequest := s.handleMembershipResolutionError(c, userID, requestedOrg, resolveErr)
		if !continueRequest {
			return
		}
		if denied {
			ctxData.OrgID = ""
			ctxData.Role = ""
		}
	} else if orgID != "" && (ctxData.OrgID != orgID || ctxData.Role != role) {
		ctxData.OrgID = orgID
		ctxData.Role = role
		if ctxData.OnboardingStatus == "CREATED" {
			ctxData.OnboardingStatus = "PROFILE_READY"
		}

		if _, ensureErr := s.userService.EnsureMembership(c.Request.Context(), users.EnsureMembershipParams{
			UserID: user.ID,
			OrgID:  orgID,
			Role:   role,
			Status: "active",
		}); ensureErr != nil {
			log.Warn().
				Err(ensureErr).
				Str("user_id", user.ID).
				Str("org_id", orgID).
				Msg("failed to backfill org membership in user-core")
		}
	}

	c.JSON(http.StatusOK, ctxData)
}

// UpdateUserProfileRequest represents a request to update user profile
type UpdateUserProfileRequest struct {
	Name           *string `json:"name,omitempty"`
	DisplayName    *string `json:"displayName,omitempty"`
	Avatar         *string `json:"avatar,omitempty"`
	FirstName      *string `json:"firstName,omitempty"`
	LastName       *string `json:"lastName,omitempty"`
	PhoneNumber    *string `json:"phoneNumber,omitempty"`
	OfficeLocation *string `json:"officeLocation,omitempty"`
	Timezone       *string `json:"timezone,omitempty"`
	Position       *string `json:"position,omitempty"`
	Department     *string `json:"department,omitempty"`
	Status         *string `json:"status,omitempty"`
}

// updateCurrentUserProfile updates the current authenticated user's profile
// PATCH /api/v1/users/me
func (s *Server) updateCurrentUserProfile(c *gin.Context) {
	userIDStr, ok := requireContextUserID(c)
	if !ok {
		return
	}

	var req UpdateUserProfileRequest
	if !bindJSONRequest(c, &req, "Invalid request body") {
		return
	}

	nameToUpdate := req.Name
	if nameToUpdate == nil || strings.TrimSpace(*nameToUpdate) == "" {
		if req.DisplayName != nil && strings.TrimSpace(*req.DisplayName) != "" {
			nameToUpdate = req.DisplayName
		} else if req.FirstName != nil || req.LastName != nil {
			first := ""
			last := ""
			if req.FirstName != nil {
				first = strings.TrimSpace(*req.FirstName)
			}
			if req.LastName != nil {
				last = strings.TrimSpace(*req.LastName)
			}
			combined := strings.TrimSpace(strings.TrimSpace(first + " " + last))
			if combined != "" {
				nameToUpdate = &combined
			}
		}
	}

	// Resolve the canonical local user row before issuing writes. The auth user
	// id can differ from the historical local user id for the same email.
	email, name, avatar := verifiedProfileFromContext(c)
	currentUser, err := s.userService.GetOrCreateUser(
		c.Request.Context(),
		userIDStr,
		email,
		name,
		avatar,
	)
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to get or create user before update")
		c.JSON(http.StatusInternalServerError, gin.H{
			"error": "Failed to update user profile",
		})
		return
	}

	// Update base user fields
	user, err := s.userService.UpdateUser(c.Request.Context(), users.UpdateUserParams{
		ID:     currentUser.ID,
		Name:   nameToUpdate,
		Avatar: req.Avatar,
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to update user")
		c.JSON(http.StatusInternalServerError, gin.H{
			"error": "Failed to update user profile",
		})
		return
	}

	metadata := map[string]any{}
	if req.FirstName != nil {
		metadata["firstName"] = *req.FirstName
	}
	if req.LastName != nil {
		metadata["lastName"] = *req.LastName
	}
	if req.Position != nil {
		metadata["position"] = *req.Position
	}
	if req.Department != nil {
		metadata["department"] = *req.Department
	}
	if req.Status != nil {
		metadata["status"] = *req.Status
	}

	hasProfileChanges := req.PhoneNumber != nil || req.OfficeLocation != nil || req.Timezone != nil || len(metadata) > 0
	if hasProfileChanges {
		if _, err := s.userService.UpdateUserProfile(c.Request.Context(), users.UpdateProfileParams{
			UserID:   currentUser.ID,
			Phone:    req.PhoneNumber,
			Location: req.OfficeLocation,
			Timezone: req.Timezone,
			Metadata: metadata,
		}); err != nil {
			log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to update extended profile")
			c.JSON(http.StatusInternalServerError, gin.H{
				"error": "Failed to update user profile",
			})
			return
		}
	}

	presenceStatus := mapUserStatusToPresence(user.Status)
	if req.Status != nil && *req.Status != "" {
		presenceStatus = *req.Status
	}

	position := ""
	if req.Position != nil {
		position = *req.Position
	}
	department := ""
	if req.Department != nil {
		department = *req.Department
	}
	phone := ""
	if req.PhoneNumber != nil {
		phone = *req.PhoneNumber
	}
	location := ""
	if req.OfficeLocation != nil {
		location = *req.OfficeLocation
	}
	timezone := ""
	if req.Timezone != nil {
		timezone = *req.Timezone
	}
	firstName := ""
	if req.FirstName != nil {
		firstName = *req.FirstName
	}
	lastName := ""
	if req.LastName != nil {
		lastName = *req.LastName
	}

	c.JSON(http.StatusOK, gin.H{
		"user": map[string]any{
			"id":             user.ID,
			"email":          user.Email,
			"name":           user.Name,
			"display_name":   user.Name,
			"avatar":         user.Avatar,
			"email_verified": user.EmailVerified,
			"status":         presenceStatus,
			"account_status": user.Status,
			"position":       position,
			"department":     department,
			"first_name":     firstName,
			"last_name":      lastName,
			"phone":          phone,
			"location":       location,
			"timezone":       timezone,
			"updated_at":     user.UpdatedAt,
			"last_login_at":  user.LastLoginAt,
		},
	})
}

func mapUserStatusToPresence(status users.UserStatus) string {
	switch status {
	case users.UserStatusInactive:
		return "offline"
	case users.UserStatusSuspended, users.UserStatusBlocked:
		return "busy"
	default:
		return "online"
	}
}

// getUserByID retrieves a user by ID (admin only)
// GET /api/v1/users/:id
func (s *Server) getUserByID(c *gin.Context) {
	if !isAdminRequest(c) && !hasServiceScope(c, "users:read:any") {
		c.JSON(http.StatusForbidden, gin.H{
			"error": "admin role required",
		})
		return
	}

	userID := c.Param("id")
	if userID == "" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "User ID required",
		})
		return
	}

	user, err := s.userService.GetUser(c.Request.Context(), userID)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("Failed to get user")
		c.JSON(http.StatusNotFound, gin.H{
			"error": "User not found",
		})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"user": map[string]any{
			"id":             user.ID,
			"email":          user.Email,
			"name":           user.Name,
			"avatar":         user.Avatar,
			"email_verified": user.EmailVerified,
			"status":         user.Status,
			"created_at":     user.CreatedAt,
			"updated_at":     user.UpdatedAt,
		},
	})
}

// getUserByEmail looks up a user by their email address.
// GET /api/v1/users/by-email/:email
func (s *Server) getUserByEmail(c *gin.Context) {
	email := c.Param("email")
	if email == "" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "email parameter required",
		})
		return
	}

	user, err := s.userService.GetUserByEmail(c.Request.Context(), email)
	if err != nil {
		log.Error().Err(err).Str("email", email).Msg("Failed to get user by email")
		c.JSON(http.StatusNotFound, gin.H{
			"error": "User not found",
		})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"user": map[string]any{
			"id":             user.ID,
			"email":          user.Email,
			"name":           user.Name,
			"avatar":         user.Avatar,
			"email_verified": user.EmailVerified,
			"status":         user.Status,
			"created_at":     user.CreatedAt,
			"updated_at":     user.UpdatedAt,
		},
	})
}

// markOnboardingComplete marks the user's onboarding as complete
// POST /api/v1/users/onboarding/complete?email=...
func (s *Server) markOnboardingComplete(c *gin.Context) {
	if currentUser, _, ok := s.ensureCanonicalCurrentUser(c); ok {
		if currentUser.ID != "" {
			err := s.userService.MarkOnboardingCompleteByID(c.Request.Context(), currentUser.ID)
			if err != nil {
				log.Error().Err(err).Str("user_id", currentUser.ID).Msg("Failed to mark onboarding complete")
				c.JSON(http.StatusInternalServerError, gin.H{
					"error": "Failed to mark onboarding complete",
				})
				return
			}

			c.JSON(http.StatusOK, gin.H{
				"success": true,
				"message": "Onboarding marked as complete",
			})
			return
		}
	}

	email := c.Query("email")
	if email == "" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "Email query parameter is required",
		})
		return
	}

	err := s.userService.MarkOnboardingComplete(c.Request.Context(), email)
	if err != nil {
		log.Error().Err(err).Str("email", email).Msg("Failed to mark onboarding complete")
		c.JSON(http.StatusInternalServerError, gin.H{
			"error": "Failed to mark onboarding complete",
		})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"success": true,
		"message": "Onboarding marked as complete",
	})
}

// ============================================
// ONBOARDING STATE ENDPOINTS (G3 + G16)
// ============================================

// getOnboardingState handles GET /api/v1/users/me/onboarding-state.
// Returns { step, state } for the authenticated user; empty step means
// "no in-flight wizard."
func (s *Server) getOnboardingState(c *gin.Context) {
	currentUser, _, ok := s.ensureCanonicalCurrentUser(c)
	if !ok {
		return
	}
	view, err := s.userService.GetOnboardingState(c.Request.Context(), currentUser.ID)
	if err != nil {
		log.Error().Err(err).Str("user_id", currentUser.ID).Msg("get onboarding state")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to read onboarding state"})
		return
	}
	c.JSON(http.StatusOK, view)
}

// putOnboardingState handles PUT /api/v1/users/me/onboarding-state.
// Body: { step: string, state?: object }. Empty step clears the column.
func (s *Server) putOnboardingState(c *gin.Context) {
	currentUser, _, ok := s.ensureCanonicalCurrentUser(c)
	if !ok {
		return
	}
	var in users.OnboardingStateView
	if err := c.ShouldBindJSON(&in); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid body: " + err.Error()})
		return
	}
	if err := s.userService.UpsertOnboardingState(c.Request.Context(), currentUser.ID, in); err != nil {
		log.Error().Err(err).Str("user_id", currentUser.ID).Msg("upsert onboarding state")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to write onboarding state"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"success": true})
}

// ============================================
// API KEY ENDPOINTS
// ============================================

// CreateAPIKeyRequest represents a request to create an API key
type CreateAPIKeyRequest struct {
	Name        string   `json:"name" binding:"required"`
	Description string   `json:"description,omitempty"`
	Scopes      []string `json:"scopes,omitempty"`
	ExpiresAt   *string  `json:"expires_at,omitempty"`
}

// createAPIKey creates a new API key for the current user
// POST /api/v1/api-keys
func (s *Server) createAPIKey(c *gin.Context) {
	userIDStr, ok := requireContextUserID(c)
	if !ok {
		return
	}

	var req CreateAPIKeyRequest
	if !bindJSONRequest(c, &req, "Invalid request body") {
		return
	}

	var expiresAt *time.Time
	if req.ExpiresAt != nil && *req.ExpiresAt != "" {
		parsed, err := time.Parse(time.RFC3339, *req.ExpiresAt)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid expires_at format, expected RFC3339"})
			return
		}
		expiresAt = &parsed
	}

	result, err := s.userService.CreateAPIKey(c.Request.Context(), users.CreateAPIKeyParams{
		UserID:      userIDStr,
		Name:        req.Name,
		Description: req.Description,
		Scopes:      req.Scopes,
		ExpiresAt:   expiresAt,
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to create API key")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to create API key"})
		return
	}

	c.JSON(http.StatusCreated, gin.H{
		"api_key": map[string]any{
			"id":          result.Key.ID,
			"name":        result.Key.Name,
			"description": result.Key.Description,
			"prefix":      result.Key.KeyPrefix,
			"scopes":      result.Key.Scopes,
			"expires_at":  result.Key.ExpiresAt,
			"created_at":  result.Key.CreatedAt,
		},
		"key": result.RawKey, // ⚠ plaintext — shown once, store securely
	})
}

// listAPIKeys lists all API keys for the current user
// GET /api/v1/api-keys
func (s *Server) listAPIKeys(c *gin.Context) {
	userIDStr, ok := requireContextUserID(c)
	if !ok {
		return
	}

	keys, err := s.userService.ListAPIKeys(c.Request.Context(), userIDStr)
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to list API keys")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to list API keys"})
		return
	}

	result := make([]map[string]any, 0, len(keys))
	for _, k := range keys {
		result = append(result, map[string]any{
			"id":           k.ID,
			"name":         k.Name,
			"description":  k.Description,
			"prefix":       k.KeyPrefix,
			"scopes":       k.Scopes,
			"expires_at":   k.ExpiresAt,
			"revoked_at":   k.RevokedAt,
			"last_used_at": k.LastUsedAt,
			"created_at":   k.CreatedAt,
		})
	}

	c.JSON(http.StatusOK, gin.H{
		"api_keys": result,
		"total":    len(result),
	})
}

// revokeAPIKey revokes an API key
// DELETE /api/v1/api-keys/:id
func (s *Server) revokeAPIKey(c *gin.Context) {
	userID, exists := c.Get("user_id")
	if !exists {
		c.JSON(http.StatusUnauthorized, gin.H{
			"error": "User ID not found in context",
		})
		return
	}

	apiKeyID := c.Param("id")
	if apiKeyID == "" {
		c.JSON(http.StatusBadRequest, gin.H{
			"error": "API key ID required",
		})
		return
	}

	userIDStr, _ := userID.(string)

	if err := s.userService.RevokeAPIKey(c.Request.Context(), userIDStr, apiKeyID); err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Str("key_id", apiKeyID).Msg("Failed to revoke API key")
		c.JSON(http.StatusNotFound, gin.H{"error": "API key not found or already revoked"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"message": "API key revoked"})
}

// ============================================
// PREFERENCES ENDPOINTS
// ============================================

// getPreferences retrieves user preferences
// GET /api/v1/preferences
func (s *Server) getPreferences(c *gin.Context) {
	userIDStr, ok := requireContextUserID(c)
	if !ok {
		return
	}

	// Aggregate preferences from the existing settings categories
	appearanceDefaults := map[string]any{"theme": "light", "colorScheme": "blue", "fontSize": "medium", "compactMode": false}
	langDefaults := map[string]any{"language": "en-US", "region": "US", "dateFormat": "MM/DD/YYYY", "timeFormat": "12h"}
	notifDefaults := map[string]any{"emailNotifications": true, "pushNotifications": true, "teamsNotifications": true, "calendarReminders": true, "quietHours": map[string]any{"enabled": false, "start": "22:00", "end": "08:00"}}

	appearance, err := s.userService.GetSettings(c.Request.Context(), userIDStr, "appearance", appearanceDefaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to get appearance settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve preferences"})
		return
	}
	langSettings, err := s.userService.GetSettings(c.Request.Context(), userIDStr, "language", langDefaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to get language settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve preferences"})
		return
	}
	notifSettings, err := s.userService.GetSettings(c.Request.Context(), userIDStr, "notifications", notifDefaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to get notification settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve preferences"})
		return
	}
	// Phase 6 selective ingest: crawl_ingest_mode defaults to "auto". The only
	// crawls this preference gates are EXPLICIT add-to-Knowledge actions (the
	// Knowledge add-source modal and the dashboard crawl composer), where
	// persisting is the expected outcome — the previous "never" default made
	// every crawl silently discard its pages ("0 docs added"). Agent browsing
	// and onboarding previews never consult this preference and stay
	// working-set-only. Users can still pick "never"/"prompt" in Settings;
	// this default only applies while the preference is genuinely unset.
	knowledgeDefaults := map[string]any{"crawlIngestMode": "auto"}
	knowledgeSettings, err := s.userService.GetSettings(c.Request.Context(), userIDStr, "knowledge", knowledgeDefaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to get knowledge settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve preferences"})
		return
	}

	// Build compact preferences object
	notifEmail, _ := notifSettings["emailNotifications"].(bool)
	notifPush, _ := notifSettings["pushNotifications"].(bool)

	c.JSON(http.StatusOK, gin.H{
		"preferences": map[string]any{
			"theme":           appearance["theme"],
			"language":        langSettings["language"],
			"timezone":        langSettings["region"],
			"crawlIngestMode": knowledgeSettings["crawlIngestMode"],
			"notifications": map[string]bool{
				"email": notifEmail,
				"push":  notifPush,
			},
		},
	})
}

// UpdatePreferencesRequest represents a request to update preferences
type UpdatePreferencesRequest struct {
	Theme         *string         `json:"theme,omitempty"`
	Language      *string         `json:"language,omitempty"`
	Timezone      *string         `json:"timezone,omitempty"`
	Notifications map[string]bool `json:"notifications,omitempty"`
	// CrawlIngestMode (Phase 6 selective ingest): auto | never | prompt. Controls
	// whether crawl/scrape results are persisted+embedded into the knowledge base.
	// The gateway reads this and sets the per-request `ingest` flag accordingly.
	CrawlIngestMode *string `json:"crawlIngestMode,omitempty"`
}

// updatePreferences updates user preferences
// PATCH /api/v1/preferences
func (s *Server) updatePreferences(c *gin.Context) {
	userIDStr, ok := requireContextUserID(c)
	if !ok {
		return
	}

	var req UpdatePreferencesRequest
	if !bindJSONRequest(c, &req, "Invalid request body") {
		return
	}

	// Route each field to the appropriate settings category
	if req.Theme != nil {
		if _, err := s.userService.UpsertSettings(c.Request.Context(), userIDStr, "appearance", map[string]any{"theme": *req.Theme}); err != nil {
			log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to update appearance preference")
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save preferences"})
			return
		}
	}
	if req.Language != nil || req.Timezone != nil {
		langPatch := map[string]any{}
		if req.Language != nil {
			langPatch["language"] = *req.Language
		}
		if req.Timezone != nil {
			langPatch["region"] = *req.Timezone
		}
		if _, err := s.userService.UpsertSettings(c.Request.Context(), userIDStr, "language", langPatch); err != nil {
			log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to update language preference")
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save preferences"})
			return
		}
	}
	if len(req.Notifications) > 0 {
		notifPatch := map[string]any{}
		for k, v := range req.Notifications {
			notifPatch[k] = v
		}
		if _, err := s.userService.UpsertSettings(c.Request.Context(), userIDStr, "notifications", notifPatch); err != nil {
			log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to update notification preference")
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save preferences"})
			return
		}
	}
	// Phase 6 selective ingest: persist crawl_ingest_mode in the "knowledge"
	// settings category. Validate against the allowed set (fail-closed to a
	// clear 400 rather than storing a value the gateway can't interpret).
	if req.CrawlIngestMode != nil {
		mode := *req.CrawlIngestMode
		if mode != "auto" && mode != "never" && mode != "prompt" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "crawlIngestMode must be one of: auto, never, prompt"})
			return
		}
		if _, err := s.userService.UpsertSettings(c.Request.Context(), userIDStr, "knowledge", map[string]any{"crawlIngestMode": mode}); err != nil {
			log.Error().Err(err).Str("user_id", userIDStr).Msg("Failed to update knowledge preference")
			c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save preferences"})
			return
		}
	}

	c.JSON(http.StatusOK, gin.H{"message": "Preferences updated successfully"})
}

// ============================================
// SETTINGS ENDPOINTS
// ============================================

// AppearanceSettings represents user appearance preferences
type AppearanceSettings struct {
	Theme       string `json:"theme"`       // "light" | "dark" | "auto"
	ColorScheme string `json:"colorScheme"` // "blue" | "green" | "purple" | "orange"
	FontSize    string `json:"fontSize"`    // "small" | "medium" | "large"
	CompactMode bool   `json:"compactMode"`
}

// LanguageSettings represents user language preferences
type LanguageSettings struct {
	Language   string `json:"language"`   // "en-US" | "nb-NO" | ...
	Region     string `json:"region"`     // "US" | "NO" | ...
	DateFormat string `json:"dateFormat"` // "MM/DD/YYYY" | "DD/MM/YYYY" | "YYYY-MM-DD"
	TimeFormat string `json:"timeFormat"` // "12h" | "24h"
}

// PrivacySettings represents user privacy preferences (aligned with frontend)
type PrivacySettings struct {
	ShareStatus      bool   `json:"shareStatus"`
	ShareActivity    bool   `json:"shareActivity"`
	AllowAnalytics   bool   `json:"allowAnalytics"`
	DataRetention    string `json:"dataRetention"` // "30days" | "90days" | "1year" | "forever"
	TelemetryEnabled bool   `json:"telemetryEnabled"`
	CrashReporting   bool   `json:"crashReporting"`
}

// QuietHours represents notification quiet hours window
type QuietHours struct {
	Enabled bool   `json:"enabled"`
	Start   string `json:"start"` // "HH:MM"
	End     string `json:"end"`   // "HH:MM"
}

// NotificationSettings represents user notification preferences (aligned with frontend)
type NotificationSettings struct {
	EmailNotifications bool       `json:"emailNotifications"`
	PushNotifications  bool       `json:"pushNotifications"`
	TeamsNotifications bool       `json:"teamsNotifications"`
	CalendarReminders  bool       `json:"calendarReminders"`
	QuietHours         QuietHours `json:"quietHours"`
}

// ============================================
// SETTINGS HELPERS
// ============================================

// structToMap converts any struct to map[string]any via JSON (used to build JSONB patches)
func structToMap(v any) (map[string]any, error) {
	data, err := json.Marshal(v)
	if err != nil {
		return nil, err
	}
	var m map[string]any
	return m, json.Unmarshal(data, &m)
}

// mapToStruct populates a typed struct from a map via JSON
func mapToStruct(m map[string]any, v any) error {
	data, err := json.Marshal(m)
	if err != nil {
		return err
	}
	return json.Unmarshal(data, v)
}

// getUserIDFromContext extracts the user_id string set by authContextMiddleware
func getUserIDFromContext(c *gin.Context) (string, bool) {
	userID, exists := c.Get("user_id")
	if !exists {
		return "", false
	}
	id, ok := userID.(string)
	return id, ok
}

// requireContextUserID reads the user_id set by authContextMiddleware,
// writing the 401/500 response and returning ok=false when it is missing or
// not a string.
func requireContextUserID(c *gin.Context) (string, bool) {
	userID, exists := c.Get("user_id")
	if !exists {
		c.JSON(http.StatusUnauthorized, gin.H{
			"error": "User ID not found in context",
		})
		return "", false
	}

	userIDStr, ok := userID.(string)
	if !ok {
		c.JSON(http.StatusInternalServerError, gin.H{
			"error": "Invalid user ID type",
		})
		return "", false
	}
	return userIDStr, true
}

// requireAuthenticatedUserID is the fail-closed guard for handlers that treat
// a missing or invalid context identity as unauthenticated.
func requireAuthenticatedUserID(c *gin.Context) (string, bool) {
	userID, ok := getUserIDFromContext(c)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
		return "", false
	}
	return userID, true
}

// bindJSONRequest decodes the JSON body into req, writing a 400 with errMsg
// and returning false on failure.
func bindJSONRequest(c *gin.Context, req any, errMsg string) bool {
	if err := c.ShouldBindJSON(req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": errMsg})
		return false
	}
	return true
}

// bindSettingsPatch decodes a settings body into req and encodes it as a
// patch map, writing the error response and returning ok=false on failure.
func bindSettingsPatch(c *gin.Context, req any) (map[string]any, bool) {
	if !bindJSONRequest(c, req, "invalid request body") {
		return nil, false
	}
	patch, err := structToMap(req)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to encode settings"})
		return nil, false
	}
	return patch, true
}

// writeSettingsResponse maps stored settings into the typed response and
// writes it, or a 500 when the stored shape no longer decodes.
func writeSettingsResponse[T any](c *gin.Context, settings map[string]any) {
	var result T
	if err := mapToStruct(settings, &result); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to decode settings"})
		return
	}
	c.JSON(http.StatusOK, result)
}

func isAdminRequest(c *gin.Context) bool {
	role, exists := c.Get("user_role")
	if !exists {
		return false
	}
	roleStr, ok := role.(string)
	if !ok {
		return false
	}
	for value := range strings.SplitSeq(roleStr, ",") {
		switch strings.ToLower(strings.TrimSpace(value)) {
		case "admin", "superadmin":
			return true
		}
	}
	return false
}

// GET /api/v1/settings/appearance
func (s *Server) getAppearanceSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"theme": "light", "colorScheme": "blue", "fontSize": "medium", "compactMode": false,
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "appearance", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get appearance settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[AppearanceSettings](c, settings)
}

// PUT /api/v1/settings/appearance
func (s *Server) updateAppearanceSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req AppearanceSettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "appearance", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update appearance settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result AppearanceSettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// GET /api/v1/settings/language
func (s *Server) getLanguageSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"language": "en-US", "region": "US", "dateFormat": "MM/DD/YYYY", "timeFormat": "12h",
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "language", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get language settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[LanguageSettings](c, settings)
}

// PUT /api/v1/settings/language
func (s *Server) updateLanguageSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req LanguageSettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "language", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update language settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result LanguageSettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// GET /api/v1/settings/privacy
func (s *Server) getPrivacySettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"shareStatus": true, "shareActivity": false, "allowAnalytics": true,
		"dataRetention": "90days", "telemetryEnabled": true, "crashReporting": true,
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "privacy", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get privacy settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[PrivacySettings](c, settings)
}

// PUT /api/v1/settings/privacy
func (s *Server) updatePrivacySettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req PrivacySettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "privacy", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update privacy settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result PrivacySettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// GET /api/v1/settings/notifications
func (s *Server) getNotificationSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"emailNotifications": true, "pushNotifications": true,
		"teamsNotifications": true, "calendarReminders": true,
		"quietHours": map[string]any{"enabled": false, "start": "22:00", "end": "08:00"},
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "notifications", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get notification settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[NotificationSettings](c, settings)
}

// PUT /api/v1/settings/notifications
func (s *Server) updateNotificationSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req NotificationSettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "notifications", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update notification settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result NotificationSettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// ============================================
// SECURITY SETTINGS
// ============================================

type SecuritySettings struct {
	TwoFactorEnabled      bool   `json:"twoFactorEnabled"`
	SessionTimeout        string `json:"sessionTimeout"` // "15min"|"1hour"|"4hours"|"1day"|"never"
	LoginAlerts           bool   `json:"loginAlerts"`
	TrustedDevicesEnabled bool   `json:"trustedDevicesEnabled"`
}

// GET /api/v1/settings/security
func (s *Server) getSecuritySettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"twoFactorEnabled": false, "sessionTimeout": "4hours",
		"loginAlerts": true, "trustedDevicesEnabled": true,
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "security", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get security settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[SecuritySettings](c, settings)
}

// PUT /api/v1/settings/security
func (s *Server) updateSecuritySettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req SecuritySettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "security", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update security settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result SecuritySettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// ============================================
// ACCESSIBILITY SETTINGS
// ============================================

type AccessibilitySettings struct {
	HighContrast             bool `json:"highContrast"`
	ReducedMotion            bool `json:"reducedMotion"`
	ScreenReaderOptimized    bool `json:"screenReaderOptimized"`
	KeyboardShortcutsEnabled bool `json:"keyboardShortcutsEnabled"`
}

// GET /api/v1/settings/accessibility
func (s *Server) getAccessibilitySettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"highContrast": false, "reducedMotion": false,
		"screenReaderOptimized": false, "keyboardShortcutsEnabled": true,
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "accessibility", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get accessibility settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[AccessibilitySettings](c, settings)
}

// PUT /api/v1/settings/accessibility
func (s *Server) updateAccessibilitySettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req AccessibilitySettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "accessibility", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update accessibility settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result AccessibilitySettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// ============================================
// AI SETTINGS
// ============================================

type AISettings struct {
	AIEnabled              bool   `json:"aiEnabled"`
	ModelPreference        string `json:"modelPreference"` // "auto"|"gpt-4"|"gpt-4o"|"claude"
	DataCollectionEnabled  bool   `json:"dataCollectionEnabled"`
	PersonalizationEnabled bool   `json:"personalizationEnabled"`
	MemoryEnabled          bool   `json:"memoryEnabled"`
}

// GET /api/v1/settings/ai
func (s *Server) getAISettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"aiEnabled": true, "modelPreference": "auto",
		"dataCollectionEnabled": true, "personalizationEnabled": true, "memoryEnabled": false,
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "ai", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get AI settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[AISettings](c, settings)
}

// PUT /api/v1/settings/ai
func (s *Server) updateAISettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req AISettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "ai", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update AI settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result AISettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// ============================================
// STORAGE SETTINGS
// ============================================

type StorageSettings struct {
	AutoSync             bool `json:"autoSync"`
	ClearCacheOnLogout   bool `json:"clearCacheOnLogout"`
	CompressionEnabled   bool `json:"compressionEnabled"`
	OfflineAccessEnabled bool `json:"offlineAccessEnabled"`
}

// GET /api/v1/settings/storage
func (s *Server) getStorageSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	defaults := map[string]any{
		"autoSync": true, "clearCacheOnLogout": false,
		"compressionEnabled": true, "offlineAccessEnabled": false,
	}
	settings, err := s.userService.GetSettings(c.Request.Context(), userID, "storage", defaults)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to get storage settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve settings"})
		return
	}
	writeSettingsResponse[StorageSettings](c, settings)
}

// PUT /api/v1/settings/storage
func (s *Server) updateStorageSettings(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req StorageSettings
	patch, ok := bindSettingsPatch(c, &req)
	if !ok {
		return
	}
	updated, err := s.userService.UpsertSettings(c.Request.Context(), userID, "storage", patch)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to update storage settings")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to save settings"})
		return
	}
	var result StorageSettings
	_ = mapToStruct(updated, &result)
	c.JSON(http.StatusOK, result)
}

// ============================================
// PROVIDER ACCOUNT ENDPOINTS
// ============================================

// LinkProviderAccountRequest is the payload when linking a social sign-in identity
type LinkProviderAccountRequest struct {
	Provider          string         `json:"provider"        binding:"required"`
	ProviderUserID    string         `json:"providerUserId"  binding:"required"`
	TenantID          string         `json:"tenantId"`
	MicrosoftTenantID string         `json:"microsoftTenantId"`
	Email             string         `json:"email"`
	EmailFromProvider string         `json:"emailFromProvider"`
	DisplayName       string         `json:"displayName"`
	ScopesGranted     []string       `json:"scopesGranted"`
	TokenRef          string         `json:"tokenRef"`
	Metadata          map[string]any `json:"metadata"`
}

type providerProfileHints struct {
	DisplayName string `json:"displayName"`
	Avatar      string `json:"avatar"`
	Locale      string `json:"locale"`
	Timezone    string `json:"timezone"`
}

// GET /api/v1/providers — list all linked OAuth providers for the authenticated user
func (s *Server) listProviderAccounts(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	accounts, err := s.userService.GetProviderAccounts(c.Request.Context(), userID)
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to list provider accounts")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to retrieve linked providers"})
		return
	}
	// Ensure we always return an array, even if empty
	if accounts == nil {
		accounts = []*users.ProviderAccount{}
	}
	c.JSON(http.StatusOK, gin.H{"providers": accounts})
}

// POST /api/v1/providers — link a social provider identity
// Internal: called from the NATS auth event handler on first social sign-in
func (s *Server) linkProviderAccount(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	var req LinkProviderAccountRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}
	account, err := s.userService.LinkProviderAccount(c.Request.Context(), users.UpsertProviderAccountParams{
		UserID:            userID,
		Provider:          req.Provider,
		ProviderUserID:    req.ProviderUserID,
		TenantID:          req.TenantID,
		MicrosoftTenantID: req.MicrosoftTenantID,
		Email:             req.Email,
		EmailFromProvider: req.EmailFromProvider,
		DisplayName:       req.DisplayName,
		ScopesGranted:     req.ScopesGranted,
		TokenRef:          req.TokenRef,
		Metadata:          req.Metadata,
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to link provider account")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to link provider"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"provider": account})
}

// EnsureMembershipRequest is used to idempotently ensure user-org membership.
type EnsureMembershipRequest struct {
	UserID string `json:"userId" binding:"required"`
	OrgID  string `json:"orgId" binding:"required"`
	Role   string `json:"role"`
	Status string `json:"status"`
}

// ensureMembership upserts a user-org membership.
// POST /api/v1/internal/memberships/ensure
func (s *Server) ensureMembership(c *gin.Context) {
	var req EnsureMembershipRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	membership, err := s.userService.EnsureMembership(c.Request.Context(), users.EnsureMembershipParams{
		UserID: req.UserID,
		OrgID:  req.OrgID,
		Role:   req.Role,
		Status: req.Status,
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", req.UserID).Str("org_id", req.OrgID).Msg("failed to ensure membership")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to ensure membership"})
		return
	}

	c.JSON(http.StatusOK, gin.H{"membership": membership})
}

// enrichUserFromProvider links or updates provider identity data for a user.
// POST /api/v1/internal/users/enrich-from-provider
func (s *Server) enrichUserFromProvider(c *gin.Context) {
	var req struct {
		UserID            string                `json:"userId" binding:"required"`
		Provider          string                `json:"provider" binding:"required"`
		ProviderUserID    string                `json:"providerUserId" binding:"required"`
		TenantID          string                `json:"tenantId"`
		MicrosoftTenantID string                `json:"microsoftTenantId"`
		Email             string                `json:"email"`
		EmailFromProvider string                `json:"emailFromProvider"`
		DisplayName       string                `json:"displayName"`
		ScopesGranted     []string              `json:"scopesGranted"`
		TokenRef          string                `json:"tokenRef"`
		Metadata          map[string]any        `json:"metadata"`
		ProfileHints      *providerProfileHints `json:"profileHints"`
	}

	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
		return
	}

	tokenRef := strings.TrimSpace(req.TokenRef)
	if tokenRef != "" {
		tokenResult, err := s.fetchAuthCoreTokenByRef(c.Request.Context(), tokenRef)
		if err != nil {
			log.Error().Err(err).Str("user_id", req.UserID).Str("token_ref", tokenRef).Msg("failed to retrieve token from auth-core")
			c.JSON(http.StatusBadGateway, gin.H{"error": "failed to retrieve token from auth-core"})
			return
		}

		if !tokenResult.Found {
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid tokenRef"})
			return
		}

		req.TokenRef = tokenResult.TokenRef
		if len(req.ScopesGranted) == 0 && strings.TrimSpace(tokenResult.Scope) != "" {
			req.ScopesGranted = strings.Fields(tokenResult.Scope)
		}
		if req.Metadata == nil {
			req.Metadata = map[string]any{}
		}
		if strings.TrimSpace(tokenResult.ExpiresAt) != "" {
			req.Metadata["authCoreTokenExpiresAt"] = tokenResult.ExpiresAt
		}
	}

	if req.ProfileHints != nil {
		if strings.TrimSpace(req.DisplayName) == "" {
			req.DisplayName = strings.TrimSpace(req.ProfileHints.DisplayName)
		}
		if req.Metadata == nil {
			req.Metadata = map[string]any{}
		}
		if strings.TrimSpace(req.ProfileHints.Avatar) != "" {
			req.Metadata["avatar"] = strings.TrimSpace(req.ProfileHints.Avatar)
		}
		if strings.TrimSpace(req.ProfileHints.Locale) != "" {
			req.Metadata["locale"] = strings.TrimSpace(req.ProfileHints.Locale)
		}
		if strings.TrimSpace(req.ProfileHints.Timezone) != "" {
			req.Metadata["timeZone"] = strings.TrimSpace(req.ProfileHints.Timezone)
		}
	}

	provider, err := s.userService.LinkProviderAccount(c.Request.Context(), users.UpsertProviderAccountParams{
		UserID:            req.UserID,
		Provider:          req.Provider,
		ProviderUserID:    req.ProviderUserID,
		TenantID:          req.TenantID,
		MicrosoftTenantID: req.MicrosoftTenantID,
		Email:             req.Email,
		EmailFromProvider: req.EmailFromProvider,
		DisplayName:       req.DisplayName,
		ScopesGranted:     req.ScopesGranted,
		TokenRef:          req.TokenRef,
		Metadata:          req.Metadata,
	})
	if err != nil {
		log.Error().Err(err).Str("user_id", req.UserID).Str("provider", req.Provider).Msg("failed to enrich user from provider")
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to enrich user from provider"})
		return
	}

	avatarHint := ""
	localeHint := ""
	timezoneHint := ""
	if req.Metadata != nil {
		if value, ok := req.Metadata["avatar"].(string); ok {
			avatarHint = strings.TrimSpace(value)
		}
		if value, ok := req.Metadata["locale"].(string); ok {
			localeHint = strings.TrimSpace(value)
		}
		if value, ok := req.Metadata["timeZone"].(string); ok {
			timezoneHint = strings.TrimSpace(value)
		}
	}

	if err := s.applySoftProviderProfileHints(
		c.Request.Context(),
		req.UserID,
		strings.TrimSpace(req.DisplayName),
		avatarHint,
		localeHint,
		timezoneHint,
	); err != nil {
		log.Warn().Err(err).Str("user_id", req.UserID).Str("provider", req.Provider).Msg("soft profile enrichment skipped")
	}

	c.JSON(http.StatusOK, gin.H{"provider": provider})
}

func (s *Server) applySoftProviderProfileHints(
	ctx context.Context,
	userID, displayName, avatar, locale, timezone string,
) error {
	user, err := s.userService.GetUser(ctx, userID)
	if err != nil {
		return err
	}

	updateUser := users.UpdateUserParams{ID: userID}
	shouldUpdateUser := false
	if displayName != "" && isPlaceholderName(user.Name) {
		name := displayName
		updateUser.Name = &name
		shouldUpdateUser = true
	}
	if avatar != "" && isPlaceholderAvatar(user.Avatar) {
		avatarCopy := avatar
		updateUser.Avatar = &avatarCopy
		shouldUpdateUser = true
	}
	if shouldUpdateUser {
		if _, err := s.userService.UpdateUser(ctx, updateUser); err != nil {
			return err
		}
	}

	profile, profileErr := s.userService.GetUserProfile(ctx, userID)
	if profileErr != nil {
		profile = nil
	}

	updateProfile := users.UpdateProfileParams{UserID: userID}
	shouldUpdateProfile := false
	if timezone != "" && (profile == nil || strings.TrimSpace(profile.Timezone) == "") {
		tz := timezone
		updateProfile.Timezone = &tz
		shouldUpdateProfile = true
	}
	if locale != "" && (profile == nil || strings.TrimSpace(profile.Language) == "") {
		lang := locale
		updateProfile.Language = &lang
		shouldUpdateProfile = true
	}
	if shouldUpdateProfile {
		if _, err := s.userService.UpdateUserProfile(ctx, updateProfile); err != nil {
			return err
		}
	}

	return nil
}

func isPlaceholderName(value string) bool {
	trimmed := strings.TrimSpace(value)
	if trimmed == "" {
		return true
	}
	normalized := strings.ToLower(trimmed)
	return normalized == "user" || normalized == "unknown user"
}

func isPlaceholderAvatar(value string) bool {
	trimmed := strings.TrimSpace(value)
	if trimmed == "" {
		return true
	}
	normalized := strings.ToLower(trimmed)
	return strings.Contains(normalized, "example.com/avatar") || strings.Contains(normalized, "placeholder")
}

func (s *Server) fetchAuthCoreTokenByRef(ctx context.Context, tokenRef string) (*authCoreTokenResponse, error) {
	authServiceURL := strings.TrimRight(strings.TrimSpace(os.Getenv("AUTH_SERVICE_URL")), "/")
	if authServiceURL == "" {
		authServiceURL = strings.TrimRight(strings.TrimSpace(os.Getenv("BETTER_AUTH_URL")), "/")
	}
	if authServiceURL == "" {
		authServiceURL = "http://localhost:3011"
	}

	if strings.TrimSpace(s.authInternalCredential.Token) == "" {
		return nil, fmt.Errorf("scoped Auth internal service credential must be configured")
	}

	body, err := json.Marshal(map[string]string{"tokenRef": tokenRef})
	if err != nil {
		return nil, fmt.Errorf("marshal auth-core token request: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, authServiceURL+"/internal/oauth/token", bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("build auth-core token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-Credential-Id", s.authInternalCredential.CredentialID)
	req.Header.Set("X-Service-Principal", s.authInternalCredential.Principal)
	req.Header.Set("X-Service-Auth", s.authInternalCredential.Token)

	client := &http.Client{Timeout: 8 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("call auth-core token endpoint: %w", err)
	}
	defer func() {
		// Body is fully read below before any use; close errors are not actionable.
		_ = resp.Body.Close()
	}()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read auth-core token response: %w", err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("auth-core token endpoint returned %d: %s", resp.StatusCode, strings.TrimSpace(string(respBody)))
	}

	var parsed authCoreTokenResponse
	if err := json.Unmarshal(respBody, &parsed); err != nil {
		return nil, fmt.Errorf("decode auth-core token response: %w", err)
	}

	if !parsed.Found && parsed.Error != "" {
		return &parsed, nil
	}

	return &parsed, nil
}

// deleteCurrentUser handles DELETE /api/v1/users/me
// It deletes the authenticated user's own account from the system.
func (s *Server) deleteCurrentUser(c *gin.Context) {
	userID, ok := requireAuthenticatedUserID(c)
	if !ok {
		return
	}
	if err := s.userService.DeleteUser(c.Request.Context(), userID); err != nil {
		log.Error().Err(err).Str("user_id", userID).Msg("failed to delete user")
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.Status(http.StatusNoContent)
}
