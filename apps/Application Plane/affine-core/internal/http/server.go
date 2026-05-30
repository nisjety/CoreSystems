package http

import (
	"context"
	"log"
	"net/http"
	"strconv"
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
	gin.SetMode(gin.ReleaseMode)
	router := gin.New()
	router.Use(gin.Recovery())

	server := &Server{
		router:      router,
		handler:     handler,
		internalKey: internalKey,
	}
	server.setupRoutes()
	server.httpServer = &http.Server{
		Addr:              ":" + strconv.Itoa(port),
		Handler:           router,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      15 * time.Second,
		IdleTimeout:       30 * time.Second,
	}
	return server
}

func (s *Server) Start() error {
	log.Printf("affine-core: HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func (s *Server) setupRoutes() {
	s.router.GET("/health", s.handler.Health)
	s.router.Any("/graphql", s.handler.ProxyRuntime)
	s.router.Any("/graphql/*path", s.handler.ProxyRuntime)
	s.router.Any("/api/health", s.handler.ProxyRuntime)
	s.router.Any("/api/sync", s.handler.ProxyRuntime)
	s.router.Any("/api/sync/*path", s.handler.ProxyRuntime)

	internal := s.router.Group("/api/v1", s.requireInternalKey())
	internal.POST("/session/exchange", s.handler.ExchangeSession)
	internal.GET("/workspaces/resolve", s.handler.ResolveWorkspace)
}

func (s *Server) requireInternalKey() gin.HandlerFunc {
	return func(c *gin.Context) {
		if s.internalKey == "" || c.GetHeader("x-internal-api-key") != s.internalKey {
			c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
			return
		}
		c.Next()
	}
}
