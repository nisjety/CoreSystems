package http

import (
	"net/http"
	"strconv"

	"github.com/gin-gonic/gin"

	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/news"
	"coresystem/apps/application-plane/information-core/internal/shipping"
	"coresystem/apps/application-plane/information-core/internal/traffic"
	"coresystem/apps/application-plane/information-core/internal/weather"
)

type Handler struct {
	cfg      config.Config
	news     *news.Service
	shipping *shipping.Service
	traffic  *traffic.Service
	weather  *weather.Service
}

func NewHandler(cfg config.Config, newsSvc *news.Service, shippingSvc *shipping.Service, trafficSvc *traffic.Service, weatherSvc *weather.Service) *Handler {
	return &Handler{cfg: cfg, news: newsSvc, shipping: shippingSvc, traffic: trafficSvc, weather: weatherSvc}
}

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status":  "ok",
		"service": h.cfg.ServiceName,
	})
}

func (h *Handler) News(c *gin.Context) {
	limit := clampInt(queryInt(c, "limit", 5), 1, 12)
	offset := maxInt(queryInt(c, "offset", 0), 0)
	maxAge := maxInt(queryInt(c, "maxAge", 24), 0)
	category := c.Query("category")

	payload, err := h.news.Latest(c.Request.Context(), h.cfg.UserAgent, category, limit, offset, maxAge)
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("news_unavailable", "News feed is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) Traffic(c *gin.Context) {
	lat := queryFloat(c, "lat", 59.9139)
	lon := queryFloat(c, "lon", 10.7522)
	radius := queryFloat(c, "radius", 35)
	search := c.Query("search")

	payload, err := h.traffic.Latest(c.Request.Context(), lat, lon, radius, search)
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("traffic_unavailable", "Traffic data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) Weather(c *gin.Context) {
	lat := queryFloat(c, "lat", 59.9139)
	lon := queryFloat(c, "lon", 10.7522)
	altitude := queryInt(c, "altitude", 90)

	payload, err := h.weather.Forecast(c.Request.Context(), h.cfg.UserAgent, lat, lon, altitude)
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("weather_unavailable", "Weather data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) Shipping(c *gin.Context) {
	trackingNumber := c.Query("trackingNumber")
	if trackingNumber == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_param", "trackingNumber query parameter is required"))
		return
	}

	payload, err := h.shipping.Track(c.Request.Context(), trackingNumber)
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("shipping_unavailable", "Shipping data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) WeatherOslo(c *gin.Context) {
	payload, err := h.weather.Oslo(c.Request.Context(), h.cfg.UserAgent)
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("weather_unavailable", "Weather data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func queryInt(c *gin.Context, key string, fallback int) int {
	raw := c.Query(key)
	if raw == "" {
		return fallback
	}
	value, err := strconv.Atoi(raw)
	if err != nil {
		return fallback
	}
	return value
}

func queryFloat(c *gin.Context, key string, fallback float64) float64 {
	raw := c.Query(key)
	if raw == "" {
		return fallback
	}
	value, err := strconv.ParseFloat(raw, 64)
	if err != nil {
		return fallback
	}
	return value
}

func clampInt(value, lower, upper int) int {
	if value < lower {
		return lower
	}
	if value > upper {
		return upper
	}
	return value
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}

func errorPayload(code, message string) gin.H {
	return gin.H{
		"error": gin.H{
			"code":    code,
			"message": message,
		},
	}
}
