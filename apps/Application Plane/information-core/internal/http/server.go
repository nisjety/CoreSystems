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

	"coresystem/apps/application-plane/information-core/internal/config"
)

type Server struct {
	httpServer *http.Server
}

func NewServer(cfg config.Config, handler *Handler) *Server {
	router := newRouter(handler, cfg.InternalAPIKey)
	return &Server{
		httpServer: &http.Server{
			Addr:              ":" + strconv.Itoa(cfg.Port),
			Handler:           router,
			ReadHeaderTimeout: 5 * time.Second,
			ReadTimeout:       15 * time.Second,
			WriteTimeout:      20 * time.Second,
			IdleTimeout:       30 * time.Second,
		},
	}
}

func (s *Server) Start() error {
	log.Printf("information-core: HTTP listening on %s", s.httpServer.Addr)
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func newRouter(handler *Handler, internalAPIKey string) *gin.Engine {
	gin.SetMode(gin.ReleaseMode)
	router := gin.New()
	router.Use(gin.Recovery())

	router.GET("/health", handler.Health)
	router.GET("/ready", handler.Ready)

	api := router.Group("/api/v1", requireInternalKey(internalAPIKey))
	api.GET("/weather", handler.Weather)
	api.GET("/weather/oslo", handler.WeatherOslo)
	api.GET("/address", handler.Address)
	api.POST("/statistics/query", handler.StatisticsQuery)
	api.GET("/statistics/metadata", handler.StatisticsMetadata)
	api.POST("/journey/plan", handler.Journey)
	api.GET("/parliament/representatives", handler.Representatives)
	api.GET("/legal/search", handler.LegalSearch)
	api.GET("/exchange", handler.Exchange)
	api.GET("/property/lookup", handler.PropertyLookup)
	api.GET("/air-quality", handler.AirQuality)
	api.GET("/roads", handler.RoadObjects)
	api.GET("/datex/situation", handler.DatexSituation)
	api.GET("/frost/observations", handler.FrostObservations)
	api.GET("/nve/avalanche-warnings", handler.AvalancheWarnings)
	api.GET("/heritage/features", handler.HeritageFeatures)
	api.GET("/air-quality/aggregate", handler.AggregateAirQuality)
	api.GET("/traffic", handler.Traffic)
	api.GET("/news", handler.News)

	return router
}

func requireInternalKey(internalAPIKey string) gin.HandlerFunc {
	return func(c *gin.Context) {
		provided := strings.TrimSpace(c.GetHeader("x-internal-api-key"))
		if internalAPIKey == "" || provided == "" {
			c.AbortWithStatusJSON(http.StatusUnauthorized, errorPayload("unauthorized", "authentication required"))
			return
		}
		if subtle.ConstantTimeCompare([]byte(provided), []byte(internalAPIKey)) != 1 {
			c.AbortWithStatusJSON(http.StatusUnauthorized, errorPayload("unauthorized", "invalid API key"))
			return
		}
		c.Next()
	}
}
