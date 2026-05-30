package http

import (
	"context"
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

func authContextMiddleware() gin.HandlerFunc {
	configuredKeys := []string{
		strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		strings.TrimSpace(os.Getenv("INTERNAL_SERVICE_SECRET")),
	}

	return func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.Method == http.MethodOptions {
			c.Next()
			return
		}

		// Internal service-to-service auth
		if reqKey := c.GetHeader("X-Internal-Api-Key"); reqKey != "" {
			for _, key := range configuredKeys {
				if key != "" && reqKey == key {
					c.Set("user_id", c.GetHeader("X-User-Id"))
					c.Set("user_email", c.GetHeader("X-User-Email"))
					c.Set("user_name", c.GetHeader("X-User-Name"))
					c.Set("auth_method", "internal")
					c.Next()
					return
				}
			}
		}

		// Bearer token auth
		if authHeader := c.GetHeader("Authorization"); strings.HasPrefix(authHeader, "Bearer ") {
			// Forward to auth-core via session.validate NATS or direct HTTP
			// For now, extract user-id from forwarded headers (frontend proxy sets these)
			if userID := c.GetHeader("X-User-Id"); userID != "" {
				c.Set("user_id", userID)
				c.Set("auth_method", "bearer")
				c.Next()
				return
			}
		}

		c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "authentication required"})
	}
}
