package http

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// Server represents the HTTP/REST server for user-service
// Phase 4 Refactor: Adding REST endpoints for frontend compatibility
// while maintaining gRPC for internal service-to-service communication
type Server struct {
	router      *gin.Engine
	server      *http.Server
	userService *users.Service
	port        string
	httpClient  *http.Client
	orgService  string
	internalKey string
}

// NewServer creates a new HTTP server
func NewServer(userService *users.Service, port string) *Server {
	router := gin.New()

	// Global middleware
	router.Use(gin.Recovery())
	router.Use(correlationMiddleware())
	router.Use(loggerMiddleware())
	router.Use(corsMiddleware())
	router.Use(authContextMiddleware())

	s := &Server{
		router:      router,
		userService: userService,
		port:        port,
		httpClient:  &http.Client{Timeout: 10 * time.Second},
		orgService:  strings.TrimRight(strings.TrimSpace(os.Getenv("ORG_SERVICE_URL")), "/"),
		internalKey: strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
	}

	if s.orgService == "" {
		s.orgService = "http://org-core:8080"
	}

	if s.internalKey == "" {
		s.internalKey = strings.TrimSpace(os.Getenv("INTERNAL_SERVICE_SECRET"))
	}

	if s.internalKey == "" {
		log.Warn().Msg("INTERNAL_API_KEY/INTERNAL_SERVICE_SECRET not set; outbound internal calls will fail")
	}

	s.setupRoutes()

	return s
}

// setupRoutes configures REST API routes
// Complements gRPC interface with frontend-facing REST endpoints
func (s *Server) setupRoutes() {
	// Health check
	s.router.GET("/health", s.healthCheck)

	// API v1
	v1 := s.router.Group("/api/v1")
	{
		// User profile endpoints (frontend-facing)
		users := v1.Group("/users")
		{
			// GET /api/v1/users/me - Get current user profile
			users.GET("/me", s.getCurrentUserProfile)

			// GET /api/v1/users/current - Alias used by the frontend proxy
			users.GET("/current", s.getCurrentUserProfile)

			// PATCH /api/v1/users/me - Update current user profile
			users.PATCH("/me", s.updateCurrentUserProfile)

			// DELETE /api/v1/users/me - Delete current user account
			users.DELETE("/me", s.deleteCurrentUser)

			// POST /api/v1/users/onboarding/complete - Mark onboarding as complete (BEFORE :id route)
			users.POST("/onboarding/complete", s.markOnboardingComplete)

			// G3 + G16: server-side onboarding state for multi-device resume.
			// GET returns { step, state } for the authenticated user; PUT
			// upserts wizard's current step + opaque state blob. The
			// `users.onboarding_complete` boolean (migration 004) remains the
			// authoritative "done?" flag; these endpoints describe in-flight
			// state. Registered BEFORE :id so the path doesn't collide.
			users.GET("/me/onboarding-state", s.getOnboardingState)
			users.PUT("/me/onboarding-state", s.putOnboardingState)

			// GET /api/v1/users/by-email/:email - Look up a user by email address (BEFORE :id catch-all)
			users.GET("/by-email/:email", s.getUserByEmail)

			// GET /api/v1/users/:id - Get user by ID (admin) (AFTER specific routes)
			users.GET("/:id", s.getUserByID)
		}

		// Session context for post-login routing
		me := v1.Group("/me")
		{
			// GET /api/v1/me/session-context
			me.GET("/session-context", s.getSessionContext)
		}

		// API Key management endpoints (frontend-facing)
		apiKeys := v1.Group("/api-keys")
		{
			// POST /api/v1/api-keys - Create new API key
			apiKeys.POST("", s.createAPIKey)

			// GET /api/v1/api-keys - List user's API keys
			apiKeys.GET("", s.listAPIKeys)

			// DELETE /api/v1/api-keys/:id - Revoke API key
			apiKeys.DELETE("/:id", s.revokeAPIKey)
		}

		// User preferences (frontend-facing)
		prefs := v1.Group("/preferences")
		{
			// GET /api/v1/preferences - Get user preferences
			prefs.GET("", s.getPreferences)

			// PATCH /api/v1/preferences - Update user preferences
			prefs.PATCH("", s.updatePreferences)
		}

		// User settings endpoints (frontend-facing, granular)
		settings := v1.Group("/settings")
		{
			// GET /api/v1/settings/appearance - Get appearance settings
			settings.GET("/appearance", s.getAppearanceSettings)
			// PUT /api/v1/settings/appearance - Update appearance settings
			settings.PUT("/appearance", s.updateAppearanceSettings)

			// GET /api/v1/settings/language - Get language settings
			settings.GET("/language", s.getLanguageSettings)
			// PUT /api/v1/settings/language - Update language settings
			settings.PUT("/language", s.updateLanguageSettings)

			// GET /api/v1/settings/privacy - Get privacy settings
			settings.GET("/privacy", s.getPrivacySettings)
			// PUT /api/v1/settings/privacy - Update privacy settings
			settings.PUT("/privacy", s.updatePrivacySettings)

			// GET /api/v1/settings/notifications - Get notification settings
			settings.GET("/notifications", s.getNotificationSettings)
			// PUT /api/v1/settings/notifications - Update notification settings
			settings.PUT("/notifications", s.updateNotificationSettings)

			// GET /api/v1/settings/security - Get security settings
			settings.GET("/security", s.getSecuritySettings)
			// PUT /api/v1/settings/security - Update security settings
			settings.PUT("/security", s.updateSecuritySettings)

			// GET /api/v1/settings/accessibility - Get accessibility settings
			settings.GET("/accessibility", s.getAccessibilitySettings)
			// PUT /api/v1/settings/accessibility - Update accessibility settings
			settings.PUT("/accessibility", s.updateAccessibilitySettings)

			// GET /api/v1/settings/ai - Get AI/Copilot settings
			settings.GET("/ai", s.getAISettings)
			// PUT /api/v1/settings/ai - Update AI/Copilot settings
			settings.PUT("/ai", s.updateAISettings)

			// GET /api/v1/settings/storage - Get storage & sync settings
			settings.GET("/storage", s.getStorageSettings)
			// PUT /api/v1/settings/storage - Update storage & sync settings
			settings.PUT("/storage", s.updateStorageSettings)
		}

		// Navbar workspace state persisted in user-core. These are intentionally
		// small JSONB-backed endpoints so the frontend can save calendar notes,
		// lightweight events, and support requests without owning persistence.
		calendar := v1.Group("/calendar")
		{
			calendar.GET("/events", s.listNavbarCalendarState)
			calendar.POST("/events", s.createNavbarCalendarEvent)
			calendar.POST("/notes", s.createNavbarCalendarNote)
		}

		support := v1.Group("/support")
		{
			support.POST("/requests", s.createNavbarSupportRequest)
		}

		// Provider accounts — linked OAuth/social sign-in identities
		providers := v1.Group("/providers")
		{
			// GET  /api/v1/providers - List linked providers for the current user
			providers.GET("", s.listProviderAccounts)
			// POST /api/v1/providers - Link a new provider identity (internal/NATS pipeline)
			providers.POST("", s.linkProviderAccount)
		}

		// Internal orchestration endpoints (idempotent hooks from auth pipeline)
		internal := v1.Group("/internal")
		{
			internal.POST("/memberships/ensure", s.ensureMembership)
			internal.POST("/users/enrich-from-provider", s.enrichUserFromProvider)
		}
	}

	log.Info().Msg("User service HTTP routes configured")
}

// Start starts the HTTP server
func (s *Server) Start() error {
	s.server = &http.Server{
		Addr:         ":" + s.port,
		Handler:      s.router,
		ReadTimeout:  15 * time.Second,
		WriteTimeout: 15 * time.Second,
		IdleTimeout:  60 * time.Second,
	}

	log.Info().Str("addr", s.server.Addr).Msg("Starting HTTP server")
	return s.server.ListenAndServe()
}

// Shutdown gracefully shuts down the HTTP server
func (s *Server) Shutdown(ctx context.Context) error {
	if s.server != nil {
		return s.server.Shutdown(ctx)
	}
	return nil
}

// healthCheck returns service health status
func (s *Server) healthCheck(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status":    "healthy",
		"service":   "user-service",
		"timestamp": time.Now().UTC().Format(time.RFC3339),
	})
}

// Middleware

// correlationMiddleware honours an inbound X-Correlation-Id, mints one if
// absent, exposes it as `correlation_id` on the gin context, and echoes it
// back in the response so velion/operator can correlate logs cross-plane.
// G15 in velion-gap.md.
func correlationMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		cid := strings.TrimSpace(c.GetHeader("X-Correlation-Id"))
		if cid == "" || len(cid) > 128 {
			cid = newCorrelationID()
		}
		c.Set("correlation_id", cid)
		c.Header("X-Correlation-Id", cid)
		c.Next()
	}
}

// newCorrelationID returns a UUID-v4 string without adding a uuid dep.
func newCorrelationID() string {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "00000000-0000-0000-0000-000000000000"
	}
	// RFC 4122 v4 layout
	b[6] = (b[6] & 0x0f) | 0x40
	b[8] = (b[8] & 0x3f) | 0x80
	return hex.EncodeToString(b[0:4]) + "-" +
		hex.EncodeToString(b[4:6]) + "-" +
		hex.EncodeToString(b[6:8]) + "-" +
		hex.EncodeToString(b[8:10]) + "-" +
		hex.EncodeToString(b[10:16])
}

func loggerMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		start := time.Now()

		c.Next()

		event := log.Info().
			Str("method", c.Request.Method).
			Str("path", c.Request.URL.Path).
			Int("status", c.Writer.Status()).
			Dur("duration", time.Since(start))
		if cid, ok := c.Get("correlation_id"); ok {
			if s, ok := cid.(string); ok && s != "" {
				event = event.Str("correlation_id", s)
			}
		}
		event.Msg("HTTP request")
	}
}

func corsMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		if origin := c.GetHeader("Origin"); origin != "" && isAllowedOrigin(origin) {
			c.Writer.Header().Set("Access-Control-Allow-Origin", origin)
			c.Writer.Header().Set("Access-Control-Allow-Credentials", "true")
			c.Writer.Header().Set("Vary", "Origin")
		}
		c.Writer.Header().Set("Access-Control-Allow-Headers", "Content-Type, Content-Length, Accept-Encoding, X-CSRF-Token, Authorization, accept, origin, Cache-Control, X-Requested-With")
		c.Writer.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS, GET, PUT, PATCH, DELETE")

		if c.Request.Method == "OPTIONS" {
			c.AbortWithStatus(http.StatusNoContent)
			return
		}

		c.Next()
	}
}

func isAllowedOrigin(origin string) bool {
	allowed := strings.TrimSpace(os.Getenv("CORS_ALLOWED_ORIGINS"))
	if allowed == "" {
		allowed = "http://localhost:3000,http://localhost:3001,http://127.0.0.1:3000,https://tools.aquatiq.com"
	}
	for _, value := range strings.Split(allowed, ",") {
		if strings.TrimSpace(value) == origin {
			return true
		}
	}
	return false
}

var bearerHTTPClient = &http.Client{Timeout: 5 * time.Second}

type bearerIdentity struct {
	userID string
	role   string
}

// resolveIdentityFromBearer validates a Bearer token with the auth-service and returns
// the associated user identity. Returns empty fields if the token is invalid/expired.
func resolveIdentityFromBearer(ctx context.Context, token, authServiceURL string) bearerIdentity {
	reqURL := authServiceURL + "/api/auth/get-session"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, reqURL, nil)
	if err != nil {
		log.Warn().Err(err).Msg("resolveUserIDFromBearer: failed to build request")
		return bearerIdentity{}
	}
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := bearerHTTPClient.Do(req)
	if err != nil {
		log.Warn().Err(err).Msg("resolveUserIDFromBearer: auth-service request failed")
		return bearerIdentity{}
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return bearerIdentity{}
	}

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return bearerIdentity{}
	}

	// Better Auth get-session response: { "session": {...}, "user": { "id": "...", ... } }
	var payload struct {
		User *struct {
			ID   string `json:"id"`
			Role string `json:"role"`
		} `json:"user"`
	}
	if err := json.Unmarshal(body, &payload); err != nil || payload.User == nil {
		return bearerIdentity{}
	}
	return bearerIdentity{userID: payload.User.ID, role: payload.User.Role}
}

func authContextMiddleware() gin.HandlerFunc {
	configuredKeys := make([]string, 0, 2)
	if value := strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")); value != "" {
		configuredKeys = append(configuredKeys, value)
	}
	if value := strings.TrimSpace(os.Getenv("INTERNAL_SERVICE_SECRET")); value != "" {
		alreadyPresent := false
		for _, existing := range configuredKeys {
			if existing == value {
				alreadyPresent = true
				break
			}
		}
		if !alreadyPresent {
			configuredKeys = append(configuredKeys, value)
		}
	}
	return func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.Method == http.MethodOptions {
			c.Next()
			return
		}

		if len(configuredKeys) == 0 {
			log.Error().Msg("Auth middleware: no internal API keys configured")
			c.AbortWithStatusJSON(http.StatusInternalServerError, gin.H{
				"error": "service auth not configured",
			})
			return
		}

		requestInternalKey := c.GetHeader("X-Internal-Api-Key")

		isMatch := false
		for _, configuredKey := range configuredKeys {
			if requestInternalKey != "" && requestInternalKey == configuredKey {
				isMatch = true
				break
			}
		}

		if !isMatch {
			// Try Authorization: Bearer <token> — validate against auth-service.
			if bearer := strings.TrimPrefix(strings.TrimSpace(c.GetHeader("Authorization")), "Bearer "); bearer != "" {
				authURL := strings.TrimRight(os.Getenv("AUTH_SERVICE_URL"), "/")
				if authURL == "" {
					authURL = "http://auth-service:3011"
				}
				if identity := resolveIdentityFromBearer(c.Request.Context(), bearer, authURL); identity.userID != "" {
					// G9: never log raw user_id at info; debug only.
					log.Debug().Str("auth_method", "bearer").Msg("Auth middleware: resolved user from Bearer")
					c.Set("user_id", identity.userID)
					if strings.TrimSpace(identity.role) != "" {
						c.Set("user_role", identity.role)
					}
					c.Next()
					return
				}
			}
			log.Warn().Msg("Auth middleware: API key mismatch or missing")
			c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{
				"error": "unauthorized",
			})
			return
		}

		log.Debug().Str("auth_method", "internal_key").Msg("Auth middleware: API key matched")

		userID := strings.TrimSpace(c.GetHeader("X-User-Id"))
		if userID != "" {
			// G9: never log raw user_id at info; debug only.
			log.Debug().Msg("Auth middleware: user_id set from forwarded header")
			c.Set("user_id", userID)
		}

		// Also extract email and name for auto-provisioning
		email := strings.TrimSpace(c.GetHeader("X-User-Email"))
		if email != "" {
			c.Set("user_email", email)
		}

		name := strings.TrimSpace(c.GetHeader("X-User-Name"))
		if name != "" {
			c.Set("user_name", name)
		}

		role := strings.TrimSpace(c.GetHeader("X-User-Role"))
		if role == "" {
			role = strings.TrimSpace(c.GetHeader("X-Auth-Role"))
		}
		if role == "" {
			role = strings.TrimSpace(c.GetHeader("X-User-Roles"))
		}
		if role != "" {
			c.Set("user_role", role)
		}

		c.Next()
	}
}
