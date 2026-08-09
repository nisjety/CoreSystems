package auth

import (
	"crypto/subtle"
	"encoding/json"
	"net/http"
	"strings"
	"time"

	"github.com/coresystem/integration-gateway/internal/audit"
)

// APIKeyAuthenticator handles API key authentication
type APIKeyAuthenticator struct {
	keys  map[string]APIKey
	audit *audit.AuditLogger
}

// APIKey represents an API key with metadata
type APIKey struct {
	Key         string     `json:"key"`
	Name        string     `json:"name"`
	Description string     `json:"description"`
	CreatedAt   time.Time  `json:"created_at"`
	ExpiresAt   *time.Time `json:"expires_at,omitempty"`
	Scopes      []string   `json:"scopes"`
	Enabled     bool       `json:"enabled"`
}

// Config holds API key authenticator configuration
type Config struct {
	Keys        []APIKey
	AuditLogger *audit.AuditLogger
}

// NewAPIKeyAuthenticator creates a new API key authenticator
func NewAPIKeyAuthenticator(cfg Config) *APIKeyAuthenticator {
	auth := &APIKeyAuthenticator{
		keys:  make(map[string]APIKey),
		audit: cfg.AuditLogger,
	}

	// Index keys by key value for fast lookup
	for _, key := range cfg.Keys {
		if key.Enabled {
			auth.keys[key.Key] = key
		}
	}

	return auth
}

// Middleware returns a middleware that validates API keys
func (a *APIKeyAuthenticator) Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Extract API key from header or query parameter
		apiKey := a.extractAPIKey(r)

		if apiKey == "" {
			a.logAuthFailure(r, "missing_api_key")
			a.respondUnauthorized(w, "API key is required")
			return
		}

		// Validate API key
		key, valid := a.validateAPIKey(apiKey)
		if !valid {
			a.logAuthFailure(r, "invalid_api_key")
			a.respondUnauthorized(w, "Invalid API key")
			return
		}

		// Check if key has expired
		if key.ExpiresAt != nil && time.Now().After(*key.ExpiresAt) {
			a.logAuthFailure(r, "expired_api_key")
			a.respondUnauthorized(w, "API key has expired")
			return
		}

		// Log successful authentication
		if a.audit != nil {
			a.audit.LogEvent(audit.AuditEvent{
				Timestamp: time.Now(),
				Action:    "api_key_authenticated",
				Actor:     key.Name,
				Resource:  r.URL.Path,
				Success:   true,
				IPAddress: getIPAddress(r),
				UserAgent: r.UserAgent(),
			})
		}

		// Add key info to request context for downstream handlers
		// (We can add this later if needed)

		next.ServeHTTP(w, r)
	})
}

// RequireScopes returns a middleware that requires specific scopes
func (a *APIKeyAuthenticator) RequireScopes(scopes ...string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			apiKey := a.extractAPIKey(r)
			key, valid := a.validateAPIKey(apiKey)

			if !valid {
				a.respondForbidden(w, "Access denied")
				return
			}

			// Check if key has required scopes
			if !a.hasScopes(key, scopes) {
				a.logAuthFailure(r, "insufficient_scopes")
				a.respondForbidden(w, "Insufficient permissions")
				return
			}

			next.ServeHTTP(w, r)
		})
	}
}

// extractAPIKey extracts the API key from request headers or query params
func (a *APIKeyAuthenticator) extractAPIKey(r *http.Request) string {
	// Try X-API-Key header first
	apiKey := r.Header.Get("X-API-Key")
	if apiKey != "" {
		return apiKey
	}

	// Try Authorization header with Bearer scheme
	authHeader := r.Header.Get("Authorization")
	if strings.HasPrefix(authHeader, "Bearer ") {
		return strings.TrimPrefix(authHeader, "Bearer ")
	}

	// Try query parameter (less secure, but sometimes needed)
	return r.URL.Query().Get("api_key")
}

// validateAPIKey validates an API key using constant-time comparison
func (a *APIKeyAuthenticator) validateAPIKey(apiKey string) (APIKey, bool) {
	for _, key := range a.keys {
		// Use constant-time comparison to prevent timing attacks
		if subtle.ConstantTimeCompare([]byte(apiKey), []byte(key.Key)) == 1 {
			return key, true
		}
	}
	return APIKey{}, false
}

// hasScopes checks if a key has all required scopes
func (a *APIKeyAuthenticator) hasScopes(key APIKey, required []string) bool {
	keyScopes := make(map[string]bool)
	for _, scope := range key.Scopes {
		keyScopes[scope] = true
	}

	for _, scope := range required {
		if !keyScopes[scope] {
			return false
		}
	}

	return true
}

// logAuthFailure logs authentication failures
func (a *APIKeyAuthenticator) logAuthFailure(r *http.Request, reason string) {
	if a.audit != nil {
		a.audit.LogAuthFailure(r, reason)
	}
}

// respondUnauthorized sends a 401 Unauthorized response
func (a *APIKeyAuthenticator) respondUnauthorized(w http.ResponseWriter, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("WWW-Authenticate", "Bearer realm=\"API\"")
	w.WriteHeader(http.StatusUnauthorized)
	json.NewEncoder(w).Encode(map[string]string{
		"error":   "unauthorized",
		"message": message,
	})
}

// respondForbidden sends a 403 Forbidden response
func (a *APIKeyAuthenticator) respondForbidden(w http.ResponseWriter, message string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusForbidden)
	json.NewEncoder(w).Encode(map[string]string{
		"error":   "forbidden",
		"message": message,
	})
}

// getIPAddress extracts the real IP address from the request
func getIPAddress(r *http.Request) string {
	// Check X-Forwarded-For header first
	xff := r.Header.Get("X-Forwarded-For")
	if xff != "" {
		// Take the first IP in the list
		ips := strings.Split(xff, ",")
		return strings.TrimSpace(ips[0])
	}

	// Check X-Real-IP header
	xri := r.Header.Get("X-Real-IP")
	if xri != "" {
		return xri
	}

	// Fall back to RemoteAddr
	return r.RemoteAddr
}

// AddKey adds a new API key at runtime
func (a *APIKeyAuthenticator) AddKey(key APIKey) {
	if key.Enabled {
		a.keys[key.Key] = key
	}
}

// RemoveKey removes an API key at runtime
func (a *APIKeyAuthenticator) RemoveKey(keyValue string) {
	delete(a.keys, keyValue)
}

// GetKeys returns all active API keys (without exposing the actual key values)
func (a *APIKeyAuthenticator) GetKeys() []APIKeyInfo {
	keys := make([]APIKeyInfo, 0, len(a.keys))
	for _, key := range a.keys {
		keys = append(keys, APIKeyInfo{
			Name:        key.Name,
			Description: key.Description,
			CreatedAt:   key.CreatedAt,
			ExpiresAt:   key.ExpiresAt,
			Scopes:      key.Scopes,
			Enabled:     key.Enabled,
		})
	}
	return keys
}

// APIKeyInfo represents API key information without the actual key
type APIKeyInfo struct {
	Name        string     `json:"name"`
	Description string     `json:"description"`
	CreatedAt   time.Time  `json:"created_at"`
	ExpiresAt   *time.Time `json:"expires_at,omitempty"`
	Scopes      []string   `json:"scopes"`
	Enabled     bool       `json:"enabled"`
}
