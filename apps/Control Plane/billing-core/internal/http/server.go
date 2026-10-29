package http

import (
	"context"
	"log"
	"net/http"
	"os"
	"strconv"
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
	serviceCredentials, credentialErr := parseServiceCredentials(os.Getenv(serviceCredentialEnv))
	if credentialErr != nil {
		log.Printf("billing-core service authentication unavailable: %v", credentialErr)
		serviceCredentials = nil
	}
	router.Use(serviceAuthMiddleware(serviceCredentials, time.Now))

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
	v1.POST("/orgs/:orgId/checkout-session/confirm", s.confirmCheckoutSession)
	v1.POST("/orgs/:orgId/deactivate", s.deactivateOrganization)
	// Nexi Checkout payment webhook. Authenticated by the per-webhook shared
	// secret Nexi echoes in the Authorization header (verified in the handler),
	// NOT by a Control Plane service principal — so its path is exempted from
	// serviceAuthMiddleware below.
	v1.POST("/webhooks/nexi", s.nexiWebhook)
}

// nexiWebhookPath is the service-principal-exempt route for the external Nexi
// payment webhook (it carries its own Authorization shared-secret instead).
const nexiWebhookPath = "/api/v1/billing/webhooks/nexi"
