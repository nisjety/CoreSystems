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
	log.Printf("conversation-core-go: HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func newRouter(handler *Handler, internalKey string) *gin.Engine {
	gin.SetMode(gin.ReleaseMode)
	router := gin.New()
	router.Use(gin.Recovery())
	router.Use(limitRequestBody(2 << 20))

	router.GET("/health", handler.Health)
	router.GET("/ready", handler.Health)

	gated := router.Group("/", requireInternalKey(internalKey))
	{
		api := gated.Group("/api/v1")
		api.GET("/inboxes", handler.ListInboxes)
		api.GET("/inboxes/:id/queue", handler.ListInboxQueue)
		api.GET("/conversations", handler.ListConversations)
		api.GET("/conversations/:id", handler.GetConversation)
		api.POST("/conversations/search", handler.SearchConversations)
		api.POST("/conversations/:id/ticket-classifications", handler.ClassifyConversationForTicket)
		api.POST("/conversations/:id/messages", handler.AddMessage)
		api.POST("/conversations/:id/notes", handler.AddNote)
		api.PATCH("/conversations/:id/status", handler.UpdateStatus)
		api.PATCH("/conversations/:id/assignment", handler.UpdateAssignment)
		api.POST("/conversations/:id/tags", handler.AddTag)
		api.DELETE("/conversations/:id/tags/:tag", handler.RemoveTag)
		api.GET("/tickets", handler.ListTickets)
		api.POST("/tickets", handler.CreateTicket)
		api.GET("/tickets/:id", handler.GetTicket)
		api.PATCH("/tickets/:id", handler.PatchTicket)
		api.POST("/tickets/:id/links", handler.LinkTicketResource)
		api.POST("/tickets/:id/macros/:macro_id/run", handler.RunTicketMacro)
		api.POST("/tickets/:id/checklists", handler.CreateTicketChecklist)
		api.PATCH("/tickets/:id/checklists/:checklist_id/items/:item_id", handler.PatchTicketChecklistItem)
		api.GET("/ticket-views", handler.ListTicketViews)
		api.POST("/ticket-views", handler.CreateTicketView)
		api.PATCH("/ticket-views/:id", handler.PatchTicketView)
		api.GET("/ticket-macros", handler.ListTicketMacros)
		api.POST("/ticket-macros", handler.CreateTicketMacro)
		api.PATCH("/ticket-macros/:id", handler.PatchTicketMacro)
		api.GET("/ticket-automation-rules", handler.ListTicketAutomationRules)
		api.POST("/ticket-automation-rules", handler.CreateTicketAutomationRule)
		api.PATCH("/ticket-automation-rules/:id", handler.PatchTicketAutomationRule)
		api.GET("/sla-policies", handler.ListSLAPolicies)
		api.POST("/sla-policies", handler.CreateSLAPolicy)
		api.PATCH("/sla-policies/:id", handler.PatchSLAPolicy)
		api.GET("/ai-actions", handler.ListAIActions)
		api.POST("/ai-actions/:id/review", handler.ReviewAIAction)
		api.POST("/ai-actions/:id/approve", handler.ApproveAIAction)
		api.POST("/ai-actions/:id/reject", handler.RejectAIAction)

		gated.POST("/internal/conversation-events", handler.IngestEvent)
		gated.GET("/internal/conversations/:id/projection", handler.GetConversation)
		gated.GET("/internal/ai-actions", handler.ListAIActions)
		gated.POST("/internal/ai-actions/:id/review", handler.ReviewAIAction)
		gated.POST("/internal/ai-actions/:id/approve", handler.ApproveAIAction)
		gated.POST("/internal/ai-actions/:id/reject", handler.RejectAIAction)
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
