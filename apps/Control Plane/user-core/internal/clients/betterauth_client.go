package clients

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"

	zlog "github.com/rs/zerolog/log"
)

// BetterAuthClient handles communication with Better Auth server
type BetterAuthClient struct {
	baseURL       string
	httpClient    *http.Client
	apiKey        string // For server-to-server authentication
	sessionCookie string // Session cookie for authenticated requests

	// sessionCache is a short-lived in-process cache keyed by session token.
	// Avoids repeated HTTP calls to auth-core for every request using the same
	// active session within the same window (TTL: sessionCacheTTL).
	sessionCache sync.Map
}

const sessionCacheTTL = 30 * time.Second

type cachedSession struct {
	user      *User
	expiresAt time.Time
}

const (
	adminAPIBasePath           = "/api/v2/auth/admin/users"
	adminCreateUserPath        = adminAPIBasePath + "/create"
	adminListUsersPath         = adminAPIBasePath + "/list"
	adminUpdateUserPath        = adminAPIBasePath + "/update"
	adminRemoveUserPath        = adminAPIBasePath + "/remove"
	adminBanUserPath           = adminAPIBasePath + "/ban"
	adminUnbanUserPath         = adminAPIBasePath + "/unban"
	adminSetRolePath           = adminAPIBasePath + "/set-role"
	adminImpersonateUserPath   = adminAPIBasePath + "/impersonate"
	adminListUserSessionsPath  = adminAPIBasePath + "/sessions"
	adminRevokeSessionPath     = adminAPIBasePath + "/revoke-session"
	adminRevokeAllSessionsPath = adminAPIBasePath + "/revoke-all-sessions"
	adminSetPasswordPath       = adminAPIBasePath + "/set-password"
)

// NewBetterAuthClient creates a new Better Auth client
func NewBetterAuthClient(baseURL, apiKey string) *BetterAuthClient {
	return &BetterAuthClient{
		baseURL: baseURL,
		apiKey:  apiKey,
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// SetServiceSecret sets the internal service secret for authenticated requests
func (c *BetterAuthClient) SetServiceSecret(secret string) {
	c.sessionCookie = secret
}

// ========== Request/Response Types ==========

// User represents a Better Auth user
type User struct {
	ID            string                 `json:"id"`
	Email         string                 `json:"email"`
	Name          string                 `json:"name"`
	EmailVerified bool                   `json:"emailVerified"`
	Image         *string                `json:"image,omitempty"`
	Role          string                 `json:"role"` // "user", "admin", "superadmin"
	Banned        bool                   `json:"banned"`
	BanReason     *string                `json:"banReason,omitempty"`
	BanExpires    *time.Time             `json:"banExpires,omitempty"`
	CreatedAt     time.Time              `json:"createdAt"`
	UpdatedAt     time.Time              `json:"updatedAt"`
	Metadata      map[string]interface{} `json:"metadata,omitempty"`
}

// CreateUserRequest represents a create user request
type CreateUserRequest struct {
	Email         string                 `json:"email"`
	Name          string                 `json:"name"`
	Password      *string                `json:"password,omitempty"`
	EmailVerified bool                   `json:"emailVerified"`
	Role          string                 `json:"role"` // "user", "admin", "superadmin"
	Image         *string                `json:"image,omitempty"`
	Metadata      map[string]interface{} `json:"metadata,omitempty"`
}

// BetterAuthResponse wraps all Better Auth API responses
type BetterAuthResponse struct {
	Data    json.RawMessage  `json:"data,omitempty"`
	Error   *BetterAuthError `json:"error,omitempty"`
	Success bool             `json:"success,omitempty"`
}

// BetterAuthError represents an error from Better Auth
type BetterAuthError struct {
	Message string `json:"message,omitempty"`
	Code    string `json:"code,omitempty"`
}

// CreateUserResponse represents a create user response
type CreateUserResponse struct {
	User User `json:"user"`
}

// UpdateUserRequest represents an update user request
type UpdateUserRequest struct {
	UserID   string                 `json:"userId"`
	Name     *string                `json:"name,omitempty"`
	Email    *string                `json:"email,omitempty"`
	Image    *string                `json:"image,omitempty"`
	Role     *string                `json:"role,omitempty"`
	Metadata map[string]interface{} `json:"metadata,omitempty"`
}

// UpdateUserResponse represents an update user response
type UpdateUserResponse struct {
	User User `json:"user"`
}

// ListUsersRequest represents a list users request
type ListUsersRequest struct {
	Limit   int     `json:"limit,omitempty"`
	Offset  int     `json:"offset,omitempty"`
	SortBy  string  `json:"sortBy,omitempty"`  // "createdAt", "name", "email"
	SortDir string  `json:"sortDir,omitempty"` // "asc", "desc"
	Search  *string `json:"search,omitempty"`  // Search in name/email
	Role    *string `json:"role,omitempty"`    // Filter by role
	Banned  *bool   `json:"banned,omitempty"`  // Filter by banned status
	UserID  *string `json:"userId,omitempty"`  // Get specific user
}

// ListUsersResponse represents a list users response
type ListUsersResponse struct {
	Users []User `json:"users"`
	Total int    `json:"total"`
}

// ChangePasswordRequest represents a change password request
type ChangePasswordRequest struct {
	UserID          string `json:"userId"`
	CurrentPassword string `json:"currentPassword"`
	NewPassword     string `json:"newPassword"`
}

// ResetPasswordRequest represents a reset password request (admin-initiated)
type ResetPasswordRequest struct {
	UserID      string `json:"userId"`
	NewPassword string `json:"newPassword"`
}

// BanUserRequest represents a ban user request
type BanUserRequest struct {
	UserID     string     `json:"userId"`
	BanReason  string     `json:"banReason"`
	BanExpires *time.Time `json:"banExpires,omitempty"` // Optional expiry date
}

// BanUserResponse represents a ban user response
type BanUserResponse struct {
	Success bool `json:"success"`
}

// UnbanUserRequest represents an unban user request
type UnbanUserRequest struct {
	UserID string `json:"userId"`
}

// UnbanUserResponse represents an unban user response
type UnbanUserResponse struct {
	Success bool `json:"success"`
}

// SetRoleRequest represents a set role request
type SetRoleRequest struct {
	UserID string `json:"userId"`
	Role   string `json:"role"` // "user", "admin", "superadmin"
}

// SetRoleResponse represents a set role response
type SetRoleResponse struct {
	Success bool `json:"success"`
}

// ImpersonateUserRequest represents an impersonate user request
type ImpersonateUserRequest struct {
	UserID string `json:"userId"`
}

// ImpersonateUserResponse represents an impersonate user response
type ImpersonateUserResponse struct {
	Token     string    `json:"token"`
	ExpiresAt time.Time `json:"expiresAt"`
}

// Session represents a Better Auth session
type Session struct {
	ID        string    `json:"id"`
	UserID    string    `json:"userId"`
	ExpiresAt time.Time `json:"expiresAt"`
	IPAddress *string   `json:"ipAddress,omitempty"`
	UserAgent *string   `json:"userAgent,omitempty"`
	CreatedAt time.Time `json:"createdAt"`
}

// ListSessionsRequest represents a list sessions request
type ListSessionsRequest struct {
	UserID string `json:"userId"`
}

// ListSessionsResponse represents a list sessions response
type ListSessionsResponse struct {
	Sessions []Session `json:"sessions"`
}

// RevokeSessionRequest represents a revoke session request
type RevokeSessionRequest struct {
	SessionID string `json:"sessionId"`
}

// RevokeSessionResponse represents a revoke session response
type RevokeSessionResponse struct {
	Success bool `json:"success"`
}

// RevokeUserSessionsRequest represents a revoke all user sessions request
type RevokeUserSessionsRequest struct {
	UserID string `json:"userId"`
}

// RevokeUserSessionsResponse represents a revoke all user sessions response
type RevokeUserSessionsResponse struct {
	Success bool `json:"success"`
	Count   int  `json:"count"` // Number of sessions revoked
}

// ErrorResponse represents a Better Auth error response
type ErrorResponse struct {
	Error   string `json:"error"`
	Message string `json:"message"`
	Code    string `json:"code,omitempty"`
}

// ========== HTTP Helper Methods ==========

// doRequest performs an HTTP request with error handling
func (c *BetterAuthClient) doRequest(ctx context.Context, method, path string, body interface{}, response interface{}) error {
	var reqBody io.Reader
	if body != nil {
		jsonData, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("failed to marshal request: %w", err)
		}
		zlog.Debug().Str("method", method).Str("path", path).Msg("Better Auth request")
		reqBody = bytes.NewBuffer(jsonData)
	}

	url := c.baseURL + path
	req, err := http.NewRequestWithContext(ctx, method, url, reqBody)
	if err != nil {
		return fmt.Errorf("failed to create request: %w", err)
	}

	// Set headers
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	if c.apiKey != "" {
		req.Header.Set("x-api-key", c.apiKey)
	}

	// Add internal service secret header for authenticated requests
	if c.sessionCookie != "" {
		req.Header.Set("X-Internal-Service-Secret", c.sessionCookie)
	} else {
		zlog.Warn().Msg("BetterAuthClient: no service secret — X-Internal-Service-Secret not set")
	}

	// Execute request
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("request failed: %w", err)
	}
	defer resp.Body.Close()

	// Read response body
	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("failed to read response: %w", err)
	}

	// Check for errors
	if resp.StatusCode >= 400 {
		zlog.Debug().Int("status", resp.StatusCode).Str("path", path).Msg("Better Auth error response")
		var errResp ErrorResponse
		if err := json.Unmarshal(respBody, &errResp); err != nil {
			return fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(respBody))
		}
		return fmt.Errorf("Better Auth error: %s (code: %s)", errResp.Message, errResp.Code)
	}

	// Parse Better Auth response wrapper
	if response != nil {
		// Try to parse as wrapped response first (admin API format)
		var wrapped BetterAuthResponse
		if err := json.Unmarshal(respBody, &wrapped); err == nil {
			// Check for error in response body
			if wrapped.Error != nil && wrapped.Error.Message != "" {
				return fmt.Errorf("Better Auth error: %s (code: %s)", wrapped.Error.Message, wrapped.Error.Code)
			}

			// If we have data field, unmarshal it into the response
			if wrapped.Data != nil {
				if err := json.Unmarshal(wrapped.Data, response); err != nil {
					return fmt.Errorf("failed to parse response data: %w", err)
				}
				return nil
			}
		}

		// Fallback: parse as direct response (non-admin API format)
		if err := json.Unmarshal(respBody, response); err != nil {
			return fmt.Errorf("failed to parse response: %w", err)
		}
	}

	return nil
}

// GetUserCached looks up a user by ID using a short in-process session cache.
// Cache entries expire after sessionCacheTTL (30s) to limit stale data risk.
func (c *BetterAuthClient) GetUserCached(ctx context.Context, userID string) (*User, error) {
	now := time.Now()
	if v, ok := c.sessionCache.Load(userID); ok {
		if entry, ok := v.(cachedSession); ok && entry.expiresAt.After(now) {
			return entry.user, nil
		}
		c.sessionCache.Delete(userID) // expired
	}

	user, err := c.GetUser(ctx, userID)
	if err != nil {
		return nil, err
	}
	c.sessionCache.Store(userID, cachedSession{user: user, expiresAt: now.Add(sessionCacheTTL)})
	return user, nil
}

// ========== User Operations ==========

// CreateUser creates a new user in Better Auth
func (c *BetterAuthClient) CreateUser(ctx context.Context, req *CreateUserRequest) (*User, error) {
	var resp CreateUserResponse
	if err := c.doRequest(ctx, http.MethodPost, adminCreateUserPath, req, &resp); err != nil {
		return nil, fmt.Errorf("failed to create user: %w", err)
	}
	return &resp.User, nil
}

// GetUser retrieves a user by ID
func (c *BetterAuthClient) GetUser(ctx context.Context, userID string) (*User, error) {
	// Better Auth list-users API
	req := &ListUsersRequest{
		UserID: &userID,
		Limit:  1,
	}

	var resp ListUsersResponse
	if err := c.doRequest(ctx, http.MethodPost, adminListUsersPath, req, &resp); err != nil {
		return nil, fmt.Errorf("failed to get user: %w", err)
	}

	if len(resp.Users) == 0 {
		return nil, fmt.Errorf("user not found")
	}

	return &resp.Users[0], nil
}

// ListUsers retrieves a paginated list of users
func (c *BetterAuthClient) ListUsers(ctx context.Context, req *ListUsersRequest) ([]User, int, error) {
	var resp ListUsersResponse
	if err := c.doRequest(ctx, http.MethodPost, adminListUsersPath, req, &resp); err != nil {
		return nil, 0, fmt.Errorf("failed to list users: %w", err)
	}
	return resp.Users, resp.Total, nil
}

// UpdateUser updates a user's information
func (c *BetterAuthClient) UpdateUser(ctx context.Context, req *UpdateUserRequest) (*User, error) {
	var resp UpdateUserResponse
	if err := c.doRequest(ctx, http.MethodPost, adminUpdateUserPath, req, &resp); err != nil {
		return nil, fmt.Errorf("failed to update user: %w", err)
	}
	return &resp.User, nil
}

// DeleteUser deletes a user (soft delete via ban)
func (c *BetterAuthClient) DeleteUser(ctx context.Context, userID string) error {
	// Better Auth uses the remove endpoint for deletion
	req := map[string]string{"userId": userID}

	var resp map[string]interface{}
	if err := c.doRequest(ctx, http.MethodPost, adminRemoveUserPath, req, &resp); err != nil {
		return fmt.Errorf("failed to delete user: %w", err)
	}

	return nil
}

// BanUser bans a user with optional expiry
func (c *BetterAuthClient) BanUser(ctx context.Context, req *BanUserRequest) error {
	var resp BanUserResponse
	if err := c.doRequest(ctx, http.MethodPost, adminBanUserPath, req, &resp); err != nil {
		return fmt.Errorf("failed to ban user: %w", err)
	}
	return nil
}

// UnbanUser unbans a user
func (c *BetterAuthClient) UnbanUser(ctx context.Context, req *UnbanUserRequest) error {
	var resp UnbanUserResponse
	if err := c.doRequest(ctx, http.MethodPost, adminUnbanUserPath, req, &resp); err != nil {
		return fmt.Errorf("failed to unban user: %w", err)
	}
	return nil
}

// SetRole sets a user's role
func (c *BetterAuthClient) SetRole(ctx context.Context, req *SetRoleRequest) error {
	var resp SetRoleResponse
	if err := c.doRequest(ctx, http.MethodPost, adminSetRolePath, req, &resp); err != nil {
		return fmt.Errorf("failed to set role: %w", err)
	}
	return nil
}

// ========== Session Operations ==========

// ImpersonateUser creates an impersonation session
func (c *BetterAuthClient) ImpersonateUser(ctx context.Context, req *ImpersonateUserRequest) (string, time.Time, error) {
	var resp ImpersonateUserResponse
	if err := c.doRequest(ctx, http.MethodPost, adminImpersonateUserPath, req, &resp); err != nil {
		return "", time.Time{}, fmt.Errorf("failed to impersonate user: %w", err)
	}
	return resp.Token, resp.ExpiresAt, nil
}

// ListSessions retrieves all sessions for a user
func (c *BetterAuthClient) ListSessions(ctx context.Context, req *ListSessionsRequest) ([]Session, error) {
	var resp ListSessionsResponse
	if err := c.doRequest(ctx, http.MethodPost, adminListUserSessionsPath, req, &resp); err != nil {
		return nil, fmt.Errorf("failed to list sessions: %w", err)
	}
	return resp.Sessions, nil
}

// RevokeSession revokes a specific session
func (c *BetterAuthClient) RevokeSession(ctx context.Context, req *RevokeSessionRequest) error {
	var resp RevokeSessionResponse
	if err := c.doRequest(ctx, http.MethodPost, adminRevokeSessionPath, req, &resp); err != nil {
		return fmt.Errorf("failed to revoke session: %w", err)
	}
	return nil
}

// RevokeUserSessions revokes all sessions for a user
func (c *BetterAuthClient) RevokeUserSessions(ctx context.Context, req *RevokeUserSessionsRequest) (int, error) {
	var resp RevokeUserSessionsResponse
	if err := c.doRequest(ctx, http.MethodPost, adminRevokeAllSessionsPath, req, &resp); err != nil {
		return 0, fmt.Errorf("failed to revoke user sessions: %w", err)
	}
	return resp.Count, nil
}

// ========== Password Management ==========

// ChangePassword changes a user's password (requires current password verification)
func (c *BetterAuthClient) ChangePassword(ctx context.Context, req *ChangePasswordRequest) error {
	if err := c.doRequest(ctx, http.MethodPost, adminSetPasswordPath, req, nil); err != nil {
		return fmt.Errorf("failed to change password: %w", err)
	}
	return nil
}

// ResetPassword resets a user's password (admin-initiated, no current password required)
func (c *BetterAuthClient) ResetPassword(ctx context.Context, req *ResetPasswordRequest) error {
	if err := c.doRequest(ctx, http.MethodPost, adminSetPasswordPath, req, nil); err != nil {
		return fmt.Errorf("failed to reset password: %w", err)
	}
	return nil
}

// ========== Health Check ==========

// Health checks if Better Auth server is reachable
func (c *BetterAuthClient) Health(ctx context.Context) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.baseURL+"/api/health", nil)
	if err != nil {
		return err
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("Better Auth server unreachable: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("Better Auth health check failed: HTTP %d", resp.StatusCode)
	}

	return nil
}
