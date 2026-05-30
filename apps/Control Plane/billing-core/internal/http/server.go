package http

import (
	"context"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/gin-gonic/gin"
)

type Server struct {
	router      *gin.Engine
	httpServer  *http.Server
	billingCore *billing.Service
}

func NewServer(port int, billingCore *billing.Service) *Server {
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(correlationMiddleware())
	router.Use(internalAuthMiddleware())

	s := &Server{
		router:      router,
		billingCore: billingCore,
	}
	s.setupRoutes()
	s.httpServer = &http.Server{
		Addr:              ":" + strconv.Itoa(port),
		Handler:           s.router,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      15 * time.Second,
		IdleTimeout:       30 * time.Second,
	}
	return s
}

func (s *Server) Start() error {
	log.Printf("billing-core HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func (s *Server) setupRoutes() {
	s.router.GET("/health", s.health)

	v1 := s.router.Group("/api/v1/billing")
	v1.GET("/orgs/:orgId/account", s.getAccount)
	v1.PUT("/orgs/:orgId/account", s.upsertAccount)
	v1.POST("/orgs/:orgId/usage", s.recordUsage)
	v1.GET("/orgs/:orgId/entitlements/:feature", s.checkEntitlement)
	v1.GET("/orgs/:orgId/quotas/:metric", s.getQuotaStatus)
	v1.POST("/orgs/:orgId/invoices", s.createInvoice)
	v1.POST("/orgs/:orgId/checkout-session", s.createCheckoutSession)
}

func internalAuthMiddleware() gin.HandlerFunc {
	configuredKeys := []string{
		strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		strings.TrimSpace(os.Getenv("INTERNAL_SERVICE_SECRET")),
	}

	return func(c *gin.Context) {
		if c.Request.URL.Path == "/health" || c.Request.Method == http.MethodOptions {
			c.Next()
			return
		}

		reqKey := strings.TrimSpace(c.GetHeader("X-Internal-Api-Key"))
		for _, key := range configuredKeys {
			if key != "" && reqKey == key {
				c.Next()
				return
			}
		}

		for _, key := range configuredKeys {
			if key != "" {
				c.AbortWithStatusJSON(http.StatusUnauthorized, gin.H{"error": "unauthorized"})
				return
			}
		}

		c.AbortWithStatusJSON(http.StatusInternalServerError, gin.H{"error": "service auth not configured"})
	}
}
