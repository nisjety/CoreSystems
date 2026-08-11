package http

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"

	internalnats "github.com/I-Dacosta/CoreSystem/apps/session-core/internal/nats"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/redis"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/service"
)

type Server struct {
	router                *gin.Engine
	server                *http.Server
	sessionService        *service.SessionService
	controlSessionService *service.ControlSessionService
	natsShared            *internalnats.SharedPublisher
	cache                 *redis.Client
	port                  string
}

func NewServer(
	sessionService *service.SessionService,
	controlSessionService *service.ControlSessionService,
	natsShared *internalnats.SharedPublisher,
	cache *redis.Client,
	port string,
) *Server {
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(correlationMiddleware())
	router.Use(loggerMiddleware())
	router.Use(corsMiddleware())
	router.Use(authContextMiddleware())

	s := &Server{
		router:                router,
		sessionService:        sessionService,
		controlSessionService: controlSessionService,
		natsShared:            natsShared,
		cache:                 cache,
		port:                  port,
	}
	s.setupRoutes()
	return s
}

func (s *Server) setupRoutes() {
	s.router.GET("/health", s.healthCheck)

	v1 := s.router.Group("/v1")
	{
		sessions := v1.Group("/sessions")
		{
			sessions.POST("", s.createSession)
			sessions.GET("/:id/state", s.getSessionState)
			sessions.GET("/:id/events", s.streamEvents) // SSE
			sessions.POST("/:id/messages", s.sendMessage)
			sessions.POST("/:id/approvals/:approval_id", s.resolveApproval)
			sessions.POST("/:id/resume", s.resumeSession)
		}

		// G36-cutover Step D: the `/v1/{plans,todos,lineage}` agent-run routes
		// were decommissioned 2026-05-12. Rust session-core's
		// `orchestration_http.rs` (port 28083:8083) is the authoritative owner
		// (see §8.24). The §10 plan anticipated a dual-write + caller-flip
		// transition; the actual greppable surface revealed zero external
		// callers, so Steps B + C collapsed into a single decommission. The
		// matching handler functions, service methods, repository files, and
		// agent-run Postgres tables are removed in the same wave.
	}

	// G10 Step 1+3: Control Session aggregator (per ADR 0002).
	// Mounted under `/api/v1/sessions/*` to distinguish it from the
	// agent-run `/v1/sessions/*` routes above (which migrate to Model Plane
	// in a later wave). Both prefixes coexist during the transition.
	apiV1 := s.router.Group("/api/v1")
	{
		controlSessions := apiV1.Group("/sessions")
		{
			controlSessions.GET("/current", s.getControlSessionCurrent)
			controlSessions.POST("/refresh", s.postControlSessionRefresh)
		}
	}
}

func (s *Server) Start() error {
	s.server = &http.Server{
		Addr:         ":" + s.port,
		Handler:      s.router,
		ReadTimeout:  15 * time.Second,
		WriteTimeout: 0, // SSE requires no write timeout
		IdleTimeout:  120 * time.Second,
	}
	log.Info().Str("port", s.port).Msg("Session-core HTTP server starting")
	return s.server.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	if s.server != nil {
		return s.server.Shutdown(ctx)
	}
	return nil
}

// --- Middleware ---

func loggerMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		start := time.Now()
		c.Next()
		log.Info().
			Str("method", c.Request.Method).
			Str("path", c.Request.URL.Path).
			Int("status", c.Writer.Status()).
			Dur("duration", time.Since(start)).
			Msg("HTTP request")
	}
}

func corsMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		if origin := c.GetHeader("Origin"); origin != "" && isAllowedOrigin(origin) {
			c.Writer.Header().Set("Access-Control-Allow-Origin", origin)
			c.Writer.Header().Set("Access-Control-Allow-Credentials", "true")
			c.Writer.Header().Set("Vary", "Origin")
		}
		c.Writer.Header().Set("Access-Control-Allow-Headers",
			"Content-Type, Authorization, X-User-Id, X-User-Email, X-User-Name, X-Internal-Api-Key, X-Idempotency-Key")
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

const serviceCredentialAudience = "session-core"

type serviceCredential struct {
	Principal string   `json:"principal"`
	Audience  string   `json:"audience"`
	Token     string   `json:"token"`
	Scopes    []string `json:"scopes"`
}

func loadServiceCredentials() []serviceCredential {
	raw := strings.TrimSpace(os.Getenv("SESSION_CORE_SERVICE_CREDENTIALS"))
	if raw == "" {
		return nil
	}

	var configured []serviceCredential
	if err := json.Unmarshal([]byte(raw), &configured); err != nil {
		log.Error().Err(err).Msg("session service credentials: invalid JSON; service-principal auth disabled")
		return nil
	}

	validated := make([]serviceCredential, 0, len(configured))
	for _, credential := range configured {
		credential.Principal = strings.TrimSpace(credential.Principal)
		credential.Audience = strings.TrimSpace(credential.Audience)
		credential.Token = strings.TrimSpace(credential.Token)
		if credential.Principal == "" || credential.Audience != serviceCredentialAudience || len(credential.Token) < 32 {
			log.Error().Str("principal", credential.Principal).Msg("session service credentials: invalid entry ignored")
			continue
		}

		scopes := make([]string, 0, len(credential.Scopes))
		for _, scope := range credential.Scopes {
			scope = strings.TrimSpace(scope)
			if scope == "sessions:read" || scope == "sessions:write" {
				scopes = append(scopes, scope)
			}
		}
		if len(scopes) == 0 {
			log.Error().Str("principal", credential.Principal).Msg("session service credentials: entry has no valid scopes")
			continue
		}
		credential.Scopes = scopes
		validated = append(validated, credential)
	}
	return validated
}

func findServiceCredential(token string, configured []serviceCredential) (serviceCredential, bool) {
	token = strings.TrimSpace(token)
	for _, credential := range configured {
		if len(token) == len(credential.Token) && subtle.ConstantTimeCompare([]byte(token), []byte(credential.Token)) == 1 {
			return credential, true
		}
	}
	return serviceCredential{}, false
}

func requiredServiceScope(method string) string {
	if method == http.MethodGet || method == http.MethodHead {
		return "sessions:read"
	}
	return "sessions:write"
}

func hasServiceScope(credential serviceCredential, required string) bool {
	for _, scope := range credential.Scopes {
		if scope == required {
			return true
		}
	}
	return false
}

// resolveIdentityFromBearer validates the bearer token against auth-core's
// live session, rather than trusting any client-supplied identity header.
// Mirrors user-core's internal/http/server.go resolveIdentityFromBearer.
func resolveIdentityFromBearer(ctx context.Context, token, authServiceURL string) bearerIdentity {
	reqURL := authServiceURL + "/api/auth/get-session"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, reqURL, nil)
	if err != nil {
		log.Warn().Err(err).Msg("resolveIdentityFromBearer: failed to build request")
		return bearerIdentity{}
	}
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := bearerHTTPClient.Do(req)
	if err != nil {
		log.Warn().Err(err).Msg("resolveIdentityFromBearer: auth-service request failed")
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
	if err := json.Unmarshal(body, &payload); err != nil || payload.User == nil || payload.User.ID == "" {
		return bearerIdentity{}
	}
	return bearerIdentity{userID: payload.User.ID, role: payload.User.Role}
}

func authContextMiddleware() gin.HandlerFunc {
	serviceCredentials := loadServiceCredentials()
	authServiceURL := strings.TrimRight(os.Getenv("AUTH_SERVICE_URL"), "/")
	if authServiceURL == "" {
		authServiceURL = "http://auth-service:3011"
	}
	nonces := newSessionDelegationNonceCache(100_000)

	return func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.Method == http.MethodOptions {
			c.Next()
			return
		}

		// Internal service-to-service auth is deliberately separate from user
		// Bearer auth. Each credential is pinned in server configuration to the
		// session-core audience, a principal, and an allow-listed scope set. The
		// fleet-wide INTERNAL_API_KEY is never accepted as delegated user identity.
		if token := c.GetHeader("X-Service-Token"); token != "" {
			credential, ok := findServiceCredential(token, serviceCredentials)
			if !ok {
				c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "invalid service credential"})
				return
			}
			if !hasServiceScope(credential, requiredServiceScope(c.Request.Method)) {
				c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "service scope denied"})
				return
			}
			delegation, ok := verifySessionServiceDelegation(c.Request, credential, nonces, time.Now())
			if !ok {
				c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "signed service delegation required"})
				return
			}
			c.Set("user_id", delegation.UserID)
			c.Set("user_email", delegation.Email)
			c.Set("user_name", delegation.Name)
			c.Set("service_principal", credential.Principal)
			c.Set("service_scopes", append([]string(nil), credential.Scopes...))
			c.Set("auth_method", "service_principal")
			c.Next()
			return
		}

		// Bearer token auth — the token itself must be validated against
		// auth-core's live session; forwarded X-User-Id/X-User-Email/X-User-Name
		// headers are NEVER trusted on their own (they are trivially forgeable
		// and were the root cause of a full session-impersonation bypass found
		// 2026-07-10 — see docs/core-research/plane-audit-2026-07-10.md finding #1).
		if authHeader := c.GetHeader("Authorization"); strings.HasPrefix(authHeader, "Bearer ") {
			token := strings.TrimSpace(strings.TrimPrefix(authHeader, "Bearer "))
			if token != "" {
				if identity := resolveIdentityFromBearer(c.Request.Context(), token, authServiceURL); identity.userID != "" {
					c.Set("user_id", identity.userID)
					if identity.role != "" {
						c.Set("user_role", identity.role)
					}
					c.Set("auth_method", "bearer")
					c.Next()
					return
				}
			}
		}

		c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
	}
}
