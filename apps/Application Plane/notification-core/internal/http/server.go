package http

import (
	"bytes"
	"context"
	"errors"
	"io"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/delegation"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/subscribers"
	"github.com/gin-gonic/gin"
)

type Server struct {
	router     *gin.Engine
	httpServer *http.Server
	handler    *Handler
	verifier   *delegation.Verifier
}

func NewServer(port int, handler *Handler, verifier *delegation.Verifier) *Server {
	router := newRouter(handler, verifier)

	return &Server{
		router:   router,
		handler:  handler,
		verifier: verifier,
		httpServer: &http.Server{
			Addr:              ":" + strconv.Itoa(port),
			Handler:           router,
			ReadHeaderTimeout: 5 * time.Second,
			ReadTimeout:       10 * time.Second,
			WriteTimeout:      15 * time.Second,
			IdleTimeout:       30 * time.Second,
		},
	}
}

func (s *Server) Start() error {
	log.Printf("notification-core: HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func newRouter(handler *Handler, verifier *delegation.Verifier) *gin.Engine {
	gin.SetMode(gin.ReleaseMode)
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(limitRequestBody(1 << 20))

	router.GET("/health", handler.Health)
	router.GET("/ready", handler.Ready)

	delegated := requireDelegation(verifier)

	internal := router.Group("/api/v1", delegated)
	internal.POST("/notification-requests", handler.CreateNotificationRequest)

	// U5-2 (ui-ux-velion-gap.md): the wire surface velion's
	// src/lib/notifications/client.ts expects. All routes are gated on the
	// internal API key; per-user routes additionally require x-user-id.
	gated := router.Group("/", delegated, requireServicePrincipal("velion-gateway"))
	{
		// Feed
		feedRoutes := gated.Group("/notifications", requireScopedIdentityHeaders(), requireActiveMembership(handler.recipients))
		feedRoutes.GET("", handler.ListFeed)
		feedRoutes.GET("/unread/count", handler.UnreadCount)
		feedRoutes.GET("/unseen/count", handler.UnseenCount)
		feedRoutes.POST("/:id/read", handler.MarkRead)
		feedRoutes.POST("/:id/seen", handler.MarkSeen)
		feedRoutes.POST("/mark-all-read", handler.MarkAllRead)
		feedRoutes.POST("/mark-all-seen", handler.MarkAllSeen)
		feedRoutes.DELETE("/:id", handler.DeleteNotification)

		// Preferences
		preferenceRoutes := gated.Group("/preferences", requireScopedIdentityHeaders(), requireActiveMembership(handler.recipients))
		preferenceRoutes.GET("", handler.ListPreferences)
		preferenceRoutes.PUT("/:eventType/:channel", handler.SetPreference)

		// Channel configs (admin-controlled — internal-only)
		channelRoutes := gated.Group("/channels/config", requireOrganizationHeader(), requireAdminRole(), requireActiveMembership(handler.recipients))
		channelRoutes.GET("", handler.ListChannelConfigs)
		channelRoutes.PATCH("/:eventType/:channel", handler.PatchChannelConfig)

	}

	return router
}

func requireActiveMembership(authorizer RecipientAuthorizer) gin.HandlerFunc {
	return func(c *gin.Context) {
		if authorizer == nil {
			c.AbortWithStatusJSON(http.StatusServiceUnavailable, gin.H{"error": "membership authority unavailable"})
			return
		}
		organizationID := strings.TrimSpace(c.GetHeader("x-org-id"))
		userID := strings.TrimSpace(c.GetHeader("x-user-id"))
		if organizationID == "" || userID == "" {
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{"error": "organization and user scope required"})
			return
		}
		if _, err := authorizer.ResolveUser(c.Request.Context(), organizationID, userID); err != nil {
			if errors.Is(err, subscribers.ErrNotFound) {
				c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "active organization membership required"})
				return
			}
			c.AbortWithStatusJSON(http.StatusServiceUnavailable, gin.H{"error": "membership authority unavailable"})
			return
		}
		c.Next()
	}
}

func requireScopedIdentityHeaders() gin.HandlerFunc {
	return func(c *gin.Context) {
		if strings.TrimSpace(c.GetHeader("x-org-id")) == "" {
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{"error": "x-org-id header required"})
			return
		}
		if strings.TrimSpace(c.GetHeader("x-user-id")) == "" {
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{"error": "x-user-id header required"})
			return
		}
		c.Next()
	}
}

func requireOrganizationHeader() gin.HandlerFunc {
	return func(c *gin.Context) {
		if strings.TrimSpace(c.GetHeader("x-org-id")) == "" {
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{"error": "x-org-id header required"})
			return
		}
		c.Next()
	}
}

func requireAdminRole() gin.HandlerFunc {
	return func(c *gin.Context) {
		principal, ok := delegatedPrincipal(c)
		if !ok || (principal.Role != "owner" && principal.Role != "admin") {
			c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "administrator role required"})
			return
		}
		c.Next()
	}
}

func requireDelegation(verifier *delegation.Verifier) gin.HandlerFunc {
	return func(c *gin.Context) {
		if verifier == nil {
			c.AbortWithStatusJSON(http.StatusServiceUnavailable, gin.H{"error": "delegation verification unavailable"})
			return
		}
		body, err := io.ReadAll(c.Request.Body)
		if err != nil {
			var maxBytesErr *http.MaxBytesError
			if errors.As(err, &maxBytesErr) {
				c.AbortWithStatusJSON(http.StatusRequestEntityTooLarge, gin.H{"error": "request body too large"})
				return
			}
			c.AbortWithStatusJSON(http.StatusBadRequest, gin.H{"error": "invalid request body"})
			return
		}
		c.Request.Body = io.NopCloser(bytes.NewReader(body))
		principal, err := verifier.Verify(c.Request, body)
		if err != nil {
			log.Printf("notification-core: unauthorized request from %s to %s", c.ClientIP(), c.Request.URL.Path)
			c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
			return
		}
		c.Set("delegation_principal", principal)
		c.Request = c.Request.WithContext(delegation.WithPrincipal(c.Request.Context(), principal))
		c.Next()
	}
}

func requireServicePrincipal(serviceID string) gin.HandlerFunc {
	return func(c *gin.Context) {
		principal, ok := delegatedPrincipal(c)
		if !ok || principal.ServiceID != serviceID {
			c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "forbidden"})
			return
		}
		c.Next()
	}
}

func delegatedPrincipal(c *gin.Context) (delegation.Principal, bool) {
	value, ok := c.Get("delegation_principal")
	if !ok {
		return delegation.Principal{}, false
	}
	principal, ok := value.(delegation.Principal)
	return principal, ok
}

func limitRequestBody(maxBytes int64) gin.HandlerFunc {
	return func(c *gin.Context) {
		if c.Request.Body != nil {
			c.Request.Body = http.MaxBytesReader(c.Writer, c.Request.Body, maxBytes)
		}

		c.Next()
	}
}
