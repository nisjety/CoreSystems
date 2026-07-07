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
	httpServer *http.Server
}

func NewServer(port int, handler *Handler, internalKey string) *Server {
	router := newRouter(handler, strings.TrimSpace(internalKey))
	return &Server{
		httpServer: &http.Server{
			Addr:              ":" + strconv.Itoa(port),
			Handler:           router,
			ReadHeaderTimeout: 5 * time.Second,
			ReadTimeout:       15 * time.Second,
			WriteTimeout:      30 * time.Second,
			IdleTimeout:       30 * time.Second,
		},
	}
}

func (s *Server) Start() error {
	log.Printf("leads-core: HTTP listening on %s", s.httpServer.Addr)
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
		api := gated.Group("/api/v1/leads")
		api.POST("/search", handler.Search)
		api.GET("/companies/:orgnr/branches", handler.Branches)
		api.GET("/companies/:orgnr/financials", handler.Financials)
		api.POST("/build_list", handler.BuildList)
		api.GET("/lists", handler.ListLists)
		api.POST("/lists", handler.CreateList)
		api.GET("/lists/:id", handler.GetList)
		api.DELETE("/lists/:id", handler.DeleteList)
		api.GET("/lists/:id/export.csv", handler.ExportCSV)

		// Provider lead sync (PERSON DATA — provider_leads only; see
		// internal/providerleads). Manual trigger + org-scoped GDPR erasure.
		gated.POST("/internal/sync/provider-leads", handler.SyncProviderLeads)
		gated.DELETE("/api/v1/provider-leads", handler.DeleteProviderLeads)
	}

	return router
}

func requireInternalKey(internalKey string) gin.HandlerFunc {
	return func(c *gin.Context) {
		provided := strings.TrimSpace(c.GetHeader("x-internal-api-key"))
		if internalKey == "" || provided == "" {
			c.AbortWithStatusJSON(http.StatusUnauthorized, errorPayload("unauthorized", "authentication required"))
			return
		}
		if subtle.ConstantTimeCompare([]byte(provided), []byte(internalKey)) != 1 {
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
