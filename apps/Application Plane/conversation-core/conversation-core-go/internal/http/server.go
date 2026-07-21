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

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
	"github.com/gin-gonic/gin"
)

type Server struct {
	router     *gin.Engine
	httpServer *http.Server
}

func NewServer(port int, handler *Handler, verifier *delegation.Verifier) *Server {
	router := newRouter(handler, verifier)
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
	log.Printf("conversation-core-go: HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func newRouter(handler *Handler, verifier *delegation.Verifier) *gin.Engine {
	gin.SetMode(gin.ReleaseMode)
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(limitRequestBody(2 << 20))

	router.GET("/health", handler.Health)
	router.GET("/ready", handler.Health)

	delegated := requireDelegation(verifier)
	gateway := router.Group("/api/v1", delegated, requireServicePrincipal("velion-gateway"), requireScopedPrincipal())
	readers := gateway.Group("/", requireAnyRole("owner", "admin", "member", "viewer"))
	agents := gateway.Group("/", requireAnyRole("owner", "admin", "member"))
	admins := gateway.Group("/", requireAnyRole("owner", "admin"))

	readers.GET("/inboxes", handler.ListInboxes)
	readers.GET("/inboxes/:id/queue", handler.ListInboxQueue)
	readers.GET("/conversations", handler.ListConversations)
	readers.GET("/conversations/:id", handler.GetConversation)
	readers.POST("/conversations/search", handler.SearchConversations)
	readers.GET("/tickets", handler.ListTickets)
	readers.GET("/tickets/:id", handler.GetTicket)
	readers.GET("/ticket-views", handler.ListTicketViews)
	readers.GET("/ticket-macros", handler.ListTicketMacros)
	readers.GET("/ticket-automation-rules", handler.ListTicketAutomationRules)
	readers.GET("/sla-policies", handler.ListSLAPolicies)
	readers.GET("/ai-actions", handler.ListAIActions)

	agents.POST("/feedback", handler.SubmitFeedback)
	agents.POST("/conversations/:id/ticket-classifications", handler.ClassifyConversationForTicket)
	agents.POST("/conversations/:id/messages", handler.AddMessage)
	agents.POST("/conversations/:id/notes", handler.AddNote)
	agents.PATCH("/conversations/:id/status", handler.UpdateStatus)
	agents.PATCH("/conversations/:id/assignment", handler.UpdateAssignment)
	agents.POST("/conversations/:id/tags", handler.AddTag)
	agents.DELETE("/conversations/:id/tags/:tag", handler.RemoveTag)
	agents.POST("/tickets", handler.CreateTicket)
	agents.PATCH("/tickets/:id", handler.PatchTicket)
	agents.POST("/tickets/:id/links", handler.LinkTicketResource)
	agents.POST("/tickets/:id/macros/:macro_id/run", handler.RunTicketMacro)
	agents.POST("/tickets/:id/checklists", handler.CreateTicketChecklist)
	agents.PATCH("/tickets/:id/checklists/:checklist_id/items/:item_id", handler.PatchTicketChecklistItem)
	agents.POST("/ai-actions/:id/approve", handler.ApproveAIAction)
	agents.POST("/ai-actions/:id/reject", handler.RejectAIAction)

	admins.POST("/ticket-views", handler.CreateTicketView)
	admins.PATCH("/ticket-views/:id", handler.PatchTicketView)
	admins.POST("/ticket-macros", handler.CreateTicketMacro)
	admins.PATCH("/ticket-macros/:id", handler.PatchTicketMacro)
	admins.POST("/ticket-automation-rules", handler.CreateTicketAutomationRule)
	admins.PATCH("/ticket-automation-rules/:id", handler.PatchTicketAutomationRule)
	admins.POST("/sla-policies", handler.CreateSLAPolicy)
	admins.PATCH("/sla-policies/:id", handler.PatchSLAPolicy)

	ingest := router.Group("/internal", delegated, requireServicePrincipal("conversation-ingest"), requireOrganizationPrincipal())
	ingest.POST("/conversation-events", handler.IngestEvent)

	return router
}

func requireDelegation(verifier *delegation.Verifier) gin.HandlerFunc {
	return func(c *gin.Context) {
		if verifier == nil {
			c.AbortWithStatusJSON(http.StatusServiceUnavailable, errorPayload("delegation_unavailable", "delegation verification unavailable"))
			return
		}
		body, err := io.ReadAll(c.Request.Body)
		if err != nil {
			var maxBytesError *http.MaxBytesError
			if errors.As(err, &maxBytesError) {
				c.AbortWithStatusJSON(http.StatusRequestEntityTooLarge, errorPayload("payload_too_large", "request body is too large"))
				return
			}
			c.AbortWithStatusJSON(http.StatusBadRequest, errorPayload("invalid_body", "request body is invalid"))
			return
		}
		c.Request.Body = io.NopCloser(bytes.NewReader(body))
		principal, err := verifier.Verify(c.Request, body)
		if err != nil {
			log.Printf("conversation-core-go: rejected delegated request from %s to %s", c.ClientIP(), c.Request.URL.Path)
			c.AbortWithStatusJSON(http.StatusUnauthorized, errorPayload("unauthorized", "authentication required"))
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
			c.AbortWithStatusJSON(http.StatusForbidden, errorPayload("forbidden", "caller is not authorized for this route"))
			return
		}
		c.Next()
	}
}

func requireScopedPrincipal() gin.HandlerFunc {
	return func(c *gin.Context) {
		principal, ok := delegatedPrincipal(c)
		if !ok || principal.UserID == "" || principal.OrganizationID == "" || principal.Role == "" {
			c.AbortWithStatusJSON(http.StatusBadRequest, errorPayload("missing_scope", "verified user, organization, and role are required"))
			return
		}
		c.Next()
	}
}

func requireOrganizationPrincipal() gin.HandlerFunc {
	return func(c *gin.Context) {
		principal, ok := delegatedPrincipal(c)
		if !ok || principal.OrganizationID == "" {
			c.AbortWithStatusJSON(http.StatusBadRequest, errorPayload("missing_scope", "verified organization is required"))
			return
		}
		c.Next()
	}
}

func requireAnyRole(roles ...string) gin.HandlerFunc {
	allowed := make(map[string]struct{}, len(roles))
	for _, role := range roles {
		allowed[strings.ToLower(strings.TrimSpace(role))] = struct{}{}
	}
	return func(c *gin.Context) {
		principal, ok := delegatedPrincipal(c)
		if !ok {
			c.AbortWithStatusJSON(http.StatusForbidden, errorPayload("forbidden", "verified role is required"))
			return
		}
		if _, exists := allowed[principal.Role]; !exists {
			c.AbortWithStatusJSON(http.StatusForbidden, errorPayload("forbidden", "role is not authorized for this route"))
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
