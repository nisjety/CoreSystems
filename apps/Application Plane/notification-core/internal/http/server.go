package http

import (
	"context"
	"crypto/subtle"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
)

type Server struct {
	router      *gin.Engine
	httpServer  *http.Server
	handler     *Handler
	internalKey string
}

func NewServer(port int, handler *Handler, internalKey string) *Server {
	router := newRouter(handler, internalKey)

	return &Server{
		router:      router,
		handler:     handler,
		internalKey: internalKey,
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

func newRouter(handler *Handler, internalKey string) *gin.Engine {
	gin.SetMode(gin.ReleaseMode)
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(limitRequestBody(1 << 20))

	router.GET("/health", handler.Health)

	// Legacy V0 dispatch (untouched).
	internal := router.Group("/api/v1", requireInternalKey(strings.TrimSpace(internalKey)))
	internal.POST("/notification-requests", handler.CreateNotificationRequest)

	// U5-2 (ui-ux-velion-gap.md): the wire surface velion's
	// src/lib/notifications/client.ts expects. All routes are gated on the
	// internal API key; per-user routes additionally require x-user-id.
	gated := router.Group("/", requireInternalKey(strings.TrimSpace(internalKey)))
	{
		// Feed
		gated.GET("/notifications", handler.ListFeed)
		gated.GET("/notifications/unread/count", handler.UnreadCount)
		gated.GET("/notifications/unseen/count", handler.UnseenCount)
		gated.POST("/notifications/:id/read", handler.MarkRead)
		gated.POST("/notifications/:id/seen", handler.MarkSeen)
		gated.POST("/notifications/mark-all-read", handler.MarkAllRead)
		gated.POST("/notifications/mark-all-seen", handler.MarkAllSeen)
		gated.DELETE("/notifications/:id", handler.DeleteNotification)

		// Preferences
		gated.GET("/preferences", handler.ListPreferences)
		gated.PUT("/preferences/:eventType/:channel", handler.SetPreference)

		// Channel configs (admin-controlled — internal-only)
		gated.GET("/channels/config", handler.ListChannelConfigs)
		gated.PATCH("/channels/config/:eventType/:channel", handler.PatchChannelConfig)

		// Subscribers (recipient identity upsert — internal-only)
		gated.POST("/internal/recipients/upsert", handler.UpsertRecipient)
	}

	return router
}

func requireInternalKey(internalKey string) gin.HandlerFunc {
	return func(c *gin.Context) {
		providedKey := strings.TrimSpace(c.GetHeader("x-internal-api-key"))
		if internalKey == "" || providedKey == "" {
			log.Printf("notification-core: unauthorized request from %s to %s", c.ClientIP(), c.Request.URL.Path)
			c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
			return
		}

		if subtle.ConstantTimeCompare([]byte(providedKey), []byte(internalKey)) != 1 {
			log.Printf("notification-core: unauthorized request from %s to %s", c.ClientIP(), c.Request.URL.Path)
			c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
			return
		}

		c.Next()
	}
}

func limitRequestBody(maxBytes int64) gin.HandlerFunc {
	return func(c *gin.Context) {
		if c.Request.Body != nil {
			c.Request.Body = http.MaxBytesReader(c.Writer, c.Request.Body, maxBytes)
		}

		c.Next()
	}
}
