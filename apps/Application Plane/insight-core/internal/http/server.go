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
	router     *gin.Engine
	httpServer *http.Server
}

func NewServer(port int, handler *Handler, internalKey string) *Server {
	router := newRouter(handler, strings.TrimSpace(internalKey))
	return &Server{
		router: router,
		httpServer: &http.Server{
			Addr:              ":" + strconv.Itoa(port),
			Handler:           router,
			ReadHeaderTimeout: 5 * time.Second,
			ReadTimeout:       15 * time.Second,
			WriteTimeout:      20 * time.Second,
			IdleTimeout:       30 * time.Second,
		},
	}
}

func (s *Server) Start() error {
	log.Printf("insight-core: HTTP listening on %s", s.httpServer.Addr)
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
	router.GET("/ready", handler.Health)

	gated := router.Group("/", requireInternalKey(internalKey))
	{
		api := gated.Group("/api/v1/insights")
		api.GET("/overview", handler.Overview)
		api.GET("/connectors", handler.ListConnectors)

		gated.POST("/internal/insight-events", handler.IngestEvent)
	}

	return router
}

func requireInternalKey(internalKey string) gin.HandlerFunc {
	return func(c *gin.Context) {
		providedKey := strings.TrimSpace(c.GetHeader("x-internal-api-key"))
		if internalKey == "" || providedKey == "" {
			c.AbortWithStatusJSON(http.StatusUnauthorized, errorPayload("unauthorized", "authentication required"))
			return
		}
		if subtle.ConstantTimeCompare([]byte(providedKey), []byte(internalKey)) != 1 {
			c.AbortWithStatusJSON(http.StatusUnauthorized, errorPayload("unauthorized", "invalid API key"))
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
