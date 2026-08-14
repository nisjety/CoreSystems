package http

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/nats"
	rediscache "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/redis"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// Server represents the HTTP/REST server for user-service
// Phase 4 Refactor: Adding REST endpoints for frontend compatibility
// while maintaining gRPC for internal service-to-service communication
type Server struct {
	router                     *gin.Engine
	server                     *http.Server
	userService                *users.Service
	removeMembershipProjection func(context.Context, string, string) error
	aclRepo                    *users.AclRepository
	spaceRepo                  *spaces.Repository
	port                       string
	httpClient                 *http.Client
	orgService                 string
	internalKey                string
	authMembershipService      string
	authMembershipToken        string
	authInternalCredential     clients.AuthInternalClientCredential
	// publisher emits aqencia.controlplane.acl.resource_grants.changed on
	// grant/revoke so the Data Plane retrieval visibility cache evicts the
	// affected (subject_id, org_id) immediately (TTL is the backstop). May be nil.
	publisher *nats.SharedPublisher
}

// SetAuthInternalCredential wires the validated, deployment-owned User Core
// identity used only for Auth Core internal OAuth contracts.
func (s *Server) SetAuthInternalCredential(credential clients.AuthInternalClientCredential) {
	s.authInternalCredential = credential
}

// SetSpaceRepository wires Control's registered-Space authority storage. It is
// intentionally separate from resource_grants: Space membership and revision
// authority must not be represented as an owner-resource ACL.
func (s *Server) SetSpaceRepository(repository *spaces.Repository) {
	s.spaceRepo = repository
}

// NewServer creates a new HTTP server. aclRepo backs the per-user authz facade
// (ListVisible/Check) consumed cross-plane by documents-api and retrieval.
func NewServer(userService *users.Service, aclRepo *users.AclRepository, sharedPublisher *nats.SharedPublisher, cache *rediscache.Client, port string) *Server {
	router := gin.New()

	// Global middleware
	router.Use(gin.Recovery())
	router.Use(correlationMiddleware())
	router.Use(loggerMiddleware())
	router.Use(corsMiddleware())
	router.Use(authContextMiddleware(cache))

	s := &Server{
		router:      router,
		userService: userService,
		aclRepo:     aclRepo,
		port:        port,
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
			CheckRedirect: func(*http.Request, []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
		orgService:            strings.TrimRight(strings.TrimSpace(os.Getenv("ORG_SERVICE_URL")), "/"),
		internalKey:           strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		authMembershipService: strings.TrimRight(strings.TrimSpace(os.Getenv("AUTH_SERVICE_URL")), "/"),
		authMembershipToken:   strings.TrimSpace(os.Getenv("USER_CORE_MEMBERSHIP_SERVICE_TOKEN")),
		publisher:             sharedPublisher,
	}
	if userService != nil {
		s.removeMembershipProjection = userService.RemoveMembership
	}

	if s.orgService == "" {
		s.orgService = "http://org-core:8080"
	}
	if s.authMembershipService == "" {
		s.authMembershipService = "http://auth-core:3011"
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
			// Operator-only delivery recovery. Static route is registered before
			// the :id catch-all and never performs a destructive purge.
			users.POST("/gdpr/operator/requeue", s.requeueGDPRDeliveries)
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

			// GDPR erasure + DSAR (admin or self-gated; call the
			// gdpr_hard_delete_user / gdpr_anonymize_user stored procedures on
			// the auth_service DB). Hard erasure is irreversible and requires
			// { "confirm": true }. DSAR export is GDPR Art. 15.
			users.DELETE("/:id/gdpr/erase", s.hardEraseUser)
			users.POST("/:id/gdpr/anonymize", s.anonymizeUser)
			users.GET("/:id/gdpr/export", s.dsarExport)
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

		// Private Inbox presentation preferences. Ticket and conversation state
		// remain owned by their respective Application/Control contracts.
		inboxWorkspace := v1.Group("/inbox-workspace")
		{
			inboxWorkspace.GET("", s.inboxWorkspaceState)
			inboxWorkspace.POST("/pins", s.updateInboxWorkspacePreference("pin"))
			inboxWorkspace.POST("/read", s.updateInboxWorkspacePreference("read"))
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

		// Internal orchestration endpoints (idempotent hooks from auth pipeline).
		// Service-principal ONLY for the WHOLE group: these mutate org membership and
		// user profile, so a plain user Bearer token must NEVER reach them — else
		// any authenticated user could self-join an arbitrary org (privilege
		// escalation). Matches the scoped service-principal auth for these hooks.
		internal := v1.Group("/internal")
		internal.Use(s.requireServicePrincipal)
		{
			internal.POST("/memberships/ensure", s.ensureMembership)
			internal.POST("/users/enrich-from-provider", s.enrichUserFromProvider)

			spaces := internal.Group("/spaces")
			{
				spaces.POST("/register", s.requireSpaceLifecycleRegistrar, s.registerSpace)
				spaces.POST("/deletion-authorizations", s.requireSpaceDeletionAuthorizer, s.authorizeSpaceDeletion)
				spaces.PUT("/deletion-policy", s.requireSpacePolicyWriter, s.upsertSpaceDeletionPolicy)
				spaces.PUT("/:space_ref/legal-hold", s.requireSpacePolicyWriter, s.applySpaceLegalHold)
				spaces.DELETE("/:space_ref/legal-hold", s.requireSpacePolicyWriter, s.releaseSpaceLegalHold)
				spaces.PUT("/:space_ref/memberships", s.requireSpaceMembershipWriter, s.replaceSpaceMemberships)
				spaces.POST("/recipient-audiences", s.requireSpaceAudiencePublisher, s.registerRecipientAudience)
				spaces.PUT("/effect-policy", s.requireSpacePolicyWriter, s.upsertSpaceEffectPolicy)
				spaces.GET("/:space_ref/membership", s.requireVerifiedSpaceResolver, s.resolveCurrentSpaceMembership)
				spaces.POST("/personal-thread-decision", s.requireVerifiedSpaceResolver, s.issuePersonalThreadDecision)
				spaces.POST("/thread-decision", s.requireVerifiedSpaceResolver, s.issueThreadDecision)
				spaces.POST("/thread-append-decision", s.requireVerifiedSpaceResolver, s.issueThreadAppendDecision)
				spaces.POST("/personal-retrieval-decision", s.requireVerifiedSpaceResolver, s.issuePersonalRetrievalDecision)
				spaces.POST("/personal-import-decision", s.requireVerifiedSpaceResolver, s.issuePersonalImportDecision)
				spaces.POST("/schedule-create-decision", s.requireVerifiedSpaceResolver, s.issueScheduleCreateDecision)
				spaces.POST("/import-execution-decision", s.requireSpaceImportReauthorizer, s.issuePersonalImportExecutionDecision)
				spaces.POST("/schedule-fire-decision", s.requireSpaceScheduleFireReauthorizer, s.issueScheduleFireDecision)
				spaces.POST("/scheduled-run-decision", s.requireSpaceScheduleFireReauthorizer, s.issueScheduledRunDecision)
			}

			// Per-user authz facade — the single internal surface Data Plane
			// services (documents-api, retrieval) call to resolve a viewer's
			// explicit resource grants. Static service credentials remain
			// contained until signed tenant+subject delegation is implemented.
			authz := internal.Group("/authz")
			authz.Use(s.requireVerifiedAuthzDelegation)
			{
				authz.GET("/visible", s.authzVisible)
				authz.GET("/check", s.authzCheck)
				// Grant write surface backing the verevonv3 ShareDialog (PR-6).
				// Disabled until verified delegation exists; resource_grants remains
				// the single authority retrieval + documents-api enforce against.
				authz.POST("/grant", s.authzGrant)
				authz.DELETE("/grant", s.authzRevoke)
				authz.GET("/grants", s.authzGrantsByResource)
			}
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

// healthCheck returns service health status. Deep check: round-trips the DB so
// an unreachable/unauthenticated database (e.g. a stale DB password) reports
// unhealthy instead of silently serving stale reads — the docker healthcheck
// hits this endpoint, so a 503 marks the container unhealthy.
func (s *Server) healthCheck(c *gin.Context) {
	ctx, cancel := context.WithTimeout(c.Request.Context(), 2*time.Second)
	defer cancel()
	if err := s.userService.Ping(ctx); err != nil {
		log.Error().Err(err).Msg("health: database ping failed")
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"status":  "unhealthy",
			"service": "user-service",
			"error":   "database unreachable",
		})
		return
	}
	delivery, err := s.userService.GDPRDeliveryHealth(ctx)
	if err != nil {
		log.Error().Err(err).Msg("health: GDPR delivery ledger unavailable")
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"status": "unhealthy", "service": "user-service", "error": "delivery ledger unavailable",
		})
		return
	}
	status := "healthy"
	if delivery.Degraded {
		status = "degraded"
	}
	c.JSON(http.StatusOK, gin.H{
		"status":        status,
		"service":       "user-service",
		"timestamp":     time.Now().UTC().Format(time.RFC3339),
		"gdpr_delivery": delivery,
	})
}

// Middleware

// correlationMiddleware honours an inbound X-Correlation-Id, mints one if
// absent, exposes it as `correlation_id` on the gin context, and echoes it
// back in the response so verevon/operator can correlate logs cross-plane.
// G15 in verevon-gap.md.
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
		allowed = "http://localhost:3000,http://localhost:3001,http://127.0.0.1:3000,https://tools.coresystem.com"
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
	email  string
	name   string
	avatar string
	role   string
}

// bearerIdentityCacheTTL bounds how long a resolved Bearer identity is served
// from cache before re-validating with Auth Core. Kept short (matches the
// control-session aggregate tolerance) so a revoked/expired token is rejected
// within the window; a token change is never cached beyond it.
const bearerIdentityCacheTTL = 30 * time.Second

// cachedIdentity is the on-cache shape (bearerIdentity has unexported fields
// that JSON cannot marshal). Keys are short to keep the Dragonfly value small.
type cachedIdentity struct {
	U string `json:"u"`
	E string `json:"e"`
	N string `json:"n"`
	A string `json:"a"`
	R string `json:"r"`
}

// resolveIdentityCached wraps resolveIdentityFromBearer with a Dragonfly
// cache keyed by a SHA-256 of the token (the raw token is never stored). Only
// successful resolutions are cached, and only for a short TTL, so this removes
// the per-request (and, on /users/me, double) Auth Core round-trip on the hot
// path without weakening verification. Cache-less and cache-error paths fall
// straight through to the authoritative resolve — never fail closed on cache.
func resolveIdentityCached(ctx context.Context, cache *rediscache.Client, token, authURL string) bearerIdentity {
	if cache == nil {
		return resolveIdentityFromBearer(ctx, token, authURL)
	}
	sum := sha256.Sum256([]byte(token))
	key := "usercore:authid:" + hex.EncodeToString(sum[:])
	if raw, err := cache.Get(ctx, key); err == nil && raw != "" {
		var ci cachedIdentity
		if json.Unmarshal([]byte(raw), &ci) == nil && ci.U != "" {
			return bearerIdentity{userID: ci.U, email: ci.E, name: ci.N, avatar: ci.A, role: ci.R}
		}
	}
	identity := resolveIdentityFromBearer(ctx, token, authURL)
	if identity.userID != "" {
		if b, err := json.Marshal(cachedIdentity{
			U: identity.userID, E: identity.email, N: identity.name, A: identity.avatar, R: identity.role,
		}); err == nil {
			_ = cache.Set(ctx, key, string(b), bearerIdentityCacheTTL)
		}
	}
	return identity
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
			ID    string `json:"id"`
			Email string `json:"email"`
			Name  string `json:"name"`
			Image string `json:"image"`
			Role  string `json:"role"`
		} `json:"user"`
	}
	if err := json.Unmarshal(body, &payload); err != nil || payload.User == nil {
		return bearerIdentity{}
	}
	return bearerIdentity{
		userID: strings.TrimSpace(payload.User.ID),
		email:  strings.ToLower(strings.TrimSpace(payload.User.Email)),
		name:   strings.TrimSpace(payload.User.Name),
		avatar: strings.TrimSpace(payload.User.Image),
		role:   strings.TrimSpace(payload.User.Role),
	}
}

func authContextMiddleware(cache *rediscache.Client) gin.HandlerFunc {
	serviceCredentials, credentialErr := parseServiceCredentials(os.Getenv("USER_CORE_SERVICE_CREDENTIALS"))
	if credentialErr != nil {
		log.Error().Err(credentialErr).Msg("Auth middleware: invalid service credential registry")
		serviceCredentials = nil
	}
	delegatedUserVerifier, delegatedUserVerifierErr := planeUserVerifierFromEnv()
	if delegatedUserVerifierErr != nil {
		log.Error().Err(delegatedUserVerifierErr).Msg("Auth middleware: delegated user proof verifier unavailable; v2 delegations fail closed")
	}
	delegationNonces := newDelegationNonceCache(100_000)
	return func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.Method == http.MethodOptions {
			c.Next()
			return
		}

		if present, authorized := authenticateServicePrincipal(c, serviceCredentials); present {
			if !authorized {
				c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "service principal not authorized"})
				return
			}
			if c.GetString("delegation_version") == "v2" {
				proof, err := delegatedUserVerifier.VerifyAuthorization(strings.TrimSpace(c.GetHeader("Authorization")))
				if err != nil || proof.UserID != c.GetString("user_id") || proof.OrgID != c.GetString("org_id") {
					c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "verified delegated user proof required"})
					return
				}
				nonceKey := c.GetString("service_id") + ":" + c.GetString("delegation_nonce")
				nonceExpiry := time.Now().UTC().Add(serviceDelegationMaxAge + serviceDelegationFutureSkew)
				if proof.Expiry.Before(nonceExpiry) {
					nonceExpiry = proof.Expiry
				}
				if !delegationNonces.Consume(nonceKey, nonceExpiry, time.Now().UTC()) {
					c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegation replay rejected"})
					return
				}
				c.Set("delegated_user_proof_verified", true)
			}
			c.Next()
			return
		}

		// User traffic is authorized only by a token Auth Core verifies.
		if bearer := strings.TrimPrefix(strings.TrimSpace(c.GetHeader("Authorization")), "Bearer "); bearer != "" {
			authURL := strings.TrimRight(os.Getenv("AUTH_SERVICE_URL"), "/")
			if authURL == "" {
				authURL = "http://auth-service:3011"
			}
			if identity := resolveIdentityCached(c.Request.Context(), cache, bearer, authURL); identity.userID != "" {
				log.Debug().Str("auth_method", "bearer").Msg("Auth middleware: resolved user from Bearer")
				c.Set("auth_method", "bearer")
				c.Set("user_id", identity.userID)
				c.Set("user_email", identity.email)
				c.Set("user_name", identity.name)
				c.Set("user_avatar", identity.avatar)
				if strings.TrimSpace(identity.role) != "" {
					c.Set("user_role", identity.role)
				}
				c.Next()
				return
			}
		}

		log.Warn().Msg("Auth middleware: verified bearer or service principal required")
		c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
	}
}
