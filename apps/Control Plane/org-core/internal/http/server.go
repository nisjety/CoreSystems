package http

import (
	"context"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/brreg"
	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/rbac"
	"github.com/gin-gonic/gin"
)

type Server struct {
	router      *gin.Engine
	httpServer  *http.Server
	orgService  *orgcore.Service
	rbacRepo    *rbac.Repository // U6-3 (ui-ux-velion-gap.md §10)
	authService string
	userService string
	httpClient  *http.Client
	brregClient *brreg.Client
}

func NewServer(port int, orgService *orgcore.Service, rbacRepo *rbac.Repository, authServiceURL, userServiceURL string) *Server {
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(correlationMiddleware())
	router.Use(internalAuthMiddleware())

	s := &Server{
		router:      router,
		orgService:  orgService,
		rbacRepo:    rbacRepo,
		authService: authServiceURL,
		userService: userServiceURL,
		httpClient:  &http.Client{Timeout: 10 * time.Second},
		brregClient: brreg.NewClient(),
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
	log.Printf("org-core HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func (s *Server) setupRoutes() {
	s.router.GET("/health", s.health)

	// V1 API routes
	v1 := s.router.Group("/api/v1")
	v1.POST("/auth/login", s.login)
	v1.GET("/users/me", s.getCurrentUser)
	v1.GET("/organizations", s.getUserOrganizations)
	v1.GET("/organizations/:id", s.getOrganization)
	v1.GET("/organizations/:id/entitlements", s.getEntitlements)
	v1.GET("/organizations/:id/members/search", s.searchMembers)
	v1.POST("/organizations", s.createOrganization)
	v1.POST("/organizations/:id/plan", s.updatePlan)
	v1.PATCH("/organizations/:id/brreg", s.verifyOrgBrreg)
	v1.GET("/brreg/search", s.searchBrreg)
	v1.GET("/brreg/:orgnr", s.lookupBrreg)

	// Frontend proxy routes (without /api/v1 prefix)
	s.router.GET("/orgs/me", s.getUserOrganizations)
	s.router.GET("/orgs", s.getUserOrganizations)
	s.router.GET("/orgs/:id", s.getOrganization)
	s.router.GET("/orgs/:id/entitlements", s.getEntitlements)
	s.router.POST("/orgs", s.createOrganization)
	s.router.POST("/orgs/:id/plan", s.updatePlan)
	s.router.PATCH("/orgs/:id/capabilities", s.updateCapabilities)
	s.router.GET("/orgs/:id/members", s.listMembers)
	s.router.POST("/orgs/:id/members/invite", s.inviteMember)
	s.router.DELETE("/orgs/:id/members/:userId", s.removeMember)
	s.router.GET("/orgs/:id/members/search", s.searchMembers)

	// U6-3 (ui-ux-velion-gap.md §10): RBAC editor surface.
	s.router.GET("/orgs/:id/roles/catalog", s.listCapabilityCatalog)
	s.router.GET("/orgs/:id/roles", s.listRoles)
	s.router.POST("/orgs/:id/roles", s.createRole)
	s.router.PATCH("/orgs/:id/roles/:roleName", s.updateRole)
	s.router.DELETE("/orgs/:id/roles/:roleName", s.deleteRole)
	s.router.PATCH("/orgs/:id/members/:userId/role", s.assignMemberRole)

	// Internal orchestration routes for zero-input enterprise onboarding
	internal := s.router.Group("/internal")
	internal.GET("/orgs/by-tenant", s.getOrganizationByTenant)
	internal.POST("/orgs/ensure-from-tenant", s.ensureOrganizationFromTenant)
	internal.POST("/orgs/:orgId/onboarding/state", s.updateOnboardingState)
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
