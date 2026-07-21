package http

import (
	"encoding/json"
	"errors"
	"net/http"
	"strconv"
	"strings"

	"github.com/gin-gonic/gin"

	"coresystem/apps/application-plane/information-core/internal/address"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/datex"
	"coresystem/apps/application-plane/information-core/internal/exchange"
	"coresystem/apps/application-plane/information-core/internal/frost"
	"coresystem/apps/application-plane/information-core/internal/geospatial"
	"coresystem/apps/application-plane/information-core/internal/journey"
	"coresystem/apps/application-plane/information-core/internal/legal"
	"coresystem/apps/application-plane/information-core/internal/news"
	"coresystem/apps/application-plane/information-core/internal/parliament"
	"coresystem/apps/application-plane/information-core/internal/statistics"
	"coresystem/apps/application-plane/information-core/internal/traffic"
	"coresystem/apps/application-plane/information-core/internal/weather"
)

type Handler struct {
	cfg     config.Config
	address *address.Service
	news    *news.Service
	traffic *traffic.Service
	weather *weather.Service
	sources NorwaySources
}

type NorwaySources struct {
	Statistics *statistics.Service
	Journey    *journey.Service
	Parliament *parliament.Service
	Legal      *legal.Service
	Exchange   *exchange.Service
	Geospatial *geospatial.Service
	Datex      *datex.Service
	Frost      *frost.Service
}

func NewHandler(cfg config.Config, addressSvc *address.Service, newsSvc *news.Service, trafficSvc *traffic.Service, weatherSvc *weather.Service) *Handler {
	return &Handler{cfg: cfg, address: addressSvc, news: newsSvc, traffic: trafficSvc, weather: weatherSvc}
}

func (h *Handler) SetNorwaySources(sources NorwaySources) {
	h.sources = sources
}

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status":  "ok",
		"service": h.cfg.ServiceName,
	})
}

func (h *Handler) Ready(c *gin.Context) {
	if strings.TrimSpace(h.cfg.InternalAPIKey) == "" {
		c.JSON(http.StatusServiceUnavailable, errorPayload("not_ready", "service credentials are not configured"))
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "ok", "service": h.cfg.ServiceName})
}

func (h *Handler) Address(c *gin.Context) {
	query := strings.TrimSpace(c.Query("q"))
	if query == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_param", "q query parameter is required"))
		return
	}
	limit, err := parseQueryInt(c, "limit", 10)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "limit must be an integer"))
		return
	}
	page, err := parseQueryInt(c, "page", 0)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "page must be an integer"))
		return
	}
	fuzzy := strings.EqualFold(strings.TrimSpace(c.Query("fuzzy")), "true")

	payload, err := h.address.Lookup(c.Request.Context(), query, limit, page, fuzzy)
	if err != nil {
		if errors.Is(err, address.ErrInvalidQuery) {
			c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "address query is outside the allowed bounds"))
			return
		}
		c.JSON(http.StatusBadGateway, errorPayload("address_unavailable", "Address data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) StatisticsQuery(c *gin.Context) {
	if h.sources.Statistics == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "SSB statistics are not configured"))
		return
	}
	var request statistics.QueryRequest
	if !decodeJSON(c, &request) {
		return
	}
	payload, err := h.sources.Statistics.Query(c.Request.Context(), request)
	if err != nil {
		h.writeSourceError(c, err, statistics.ErrInvalidQuery, "statistics_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) StatisticsMetadata(c *gin.Context) {
	if h.sources.Statistics == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "SSB statistics are not configured"))
		return
	}
	tableID := strings.TrimSpace(c.Query("table"))
	payload, err := h.sources.Statistics.Metadata(c.Request.Context(), tableID)
	if err != nil {
		h.writeSourceError(c, err, statistics.ErrInvalidQuery, "statistics_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) Journey(c *gin.Context) {
	if h.sources.Journey == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Entur journey planning is not configured"))
		return
	}
	var request journey.PlanRequest
	if !decodeJSON(c, &request) {
		return
	}
	payload, err := h.sources.Journey.Plan(c.Request.Context(), request)
	if err != nil {
		h.writeSourceError(c, err, journey.ErrInvalidRequest, "journey_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) Representatives(c *gin.Context) {
	if h.sources.Parliament == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Storting data is not configured"))
		return
	}
	payload, err := h.sources.Parliament.CurrentRepresentatives(c.Request.Context())
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("parliament_unavailable", "Parliament data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) LegalSearch(c *gin.Context) {
	if h.sources.Legal == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Lovdata is not configured"))
		return
	}
	query := strings.TrimSpace(c.Query("q"))
	terms := strings.Fields(query)
	limit, err := parseQueryInt(c, "limit", 10)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "limit must be an integer"))
		return
	}
	offset, err := parseQueryInt(c, "offset", 0)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "offset must be an integer"))
		return
	}
	payload, err := h.sources.Legal.Search(c.Request.Context(), legal.Request{Terms: terms, Base: strings.TrimSpace(c.Query("base")), Limit: limit, Offset: offset})
	if err != nil {
		if errors.Is(err, legal.ErrNotConfigured) {
			c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Lovdata API credentials are not configured"))
			return
		}
		h.writeSourceError(c, err, legal.ErrInvalidRequest, "legal_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) Exchange(c *gin.Context) {
	if h.sources.Exchange == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Norges Bank data is not configured"))
		return
	}
	lastN, err := parseQueryInt(c, "lastN", 0)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "lastN must be an integer"))
		return
	}
	payload, err := h.sources.Exchange.Series(c.Request.Context(), exchange.Request{Series: strings.TrimSpace(c.Query("series")), StartPeriod: strings.TrimSpace(c.Query("startPeriod")), EndPeriod: strings.TrimSpace(c.Query("endPeriod")), LastNObservations: lastN})
	if err != nil {
		h.writeSourceError(c, err, exchange.ErrInvalidRequest, "exchange_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) PropertyLookup(c *gin.Context) {
	if h.sources.Geospatial == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Property location is not configured"))
		return
	}
	payload, err := h.sources.Geospatial.PropertyLookup(c.Request.Context(), geospatial.PropertyRequest{MatrikkelNumber: c.Query("matrikkelnummer")})
	if err != nil {
		h.writeSourceError(c, err, geospatial.ErrInvalidRequest, "property_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) AirQuality(c *gin.Context) {
	if h.sources.Geospatial == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Air quality is not configured"))
		return
	}
	lat, err := parseQueryFloat(c, "lat")
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "lat must be a number"))
		return
	}
	lon, err := parseQueryFloat(c, "lon")
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "lon must be a number"))
		return
	}
	radius, err := parseQueryInt(c, "radius", 10)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "radius must be an integer"))
		return
	}
	payload, err := h.sources.Geospatial.AirQuality(c.Request.Context(), lat, lon, radius)
	if err != nil {
		h.writeSourceError(c, err, geospatial.ErrInvalidRequest, "air_quality_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) RoadObjects(c *gin.Context) {
	if h.sources.Geospatial == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Road data is not configured"))
		return
	}
	objectType, err := parseQueryInt(c, "objectType", 0)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "objectType must be an integer"))
		return
	}
	limit, err := parseQueryInt(c, "limit", 50)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "limit must be an integer"))
		return
	}
	payload, err := h.sources.Geospatial.RoadObjects(c.Request.Context(), geospatial.RoadRequest{ObjectType: objectType, Municipality: c.Query("municipality"), Limit: limit})
	if err != nil {
		h.writeSourceError(c, err, geospatial.ErrInvalidRequest, "roads_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) DatexSituation(c *gin.Context) {
	if h.sources.Datex == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "DATEX II is not configured"))
		return
	}
	payload, err := h.sources.Datex.PullSituation(c.Request.Context())
	if err != nil {
		if errors.Is(err, datex.ErrNotConfigured) {
			c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "DATEX II registration is not configured"))
			return
		}
		c.JSON(http.StatusBadGateway, errorPayload("datex_unavailable", "DATEX II data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) FrostObservations(c *gin.Context) {
	if h.sources.Frost == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Frost is not configured"))
		return
	}
	limit, err := parseQueryInt(c, "limit", 0)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "limit must be an integer"))
		return
	}
	payload, err := h.sources.Frost.Observations(c.Request.Context(), frost.Request{Sources: c.Query("sources"), Elements: c.Query("elements"), ReferenceTime: c.Query("referencetime"), Limit: limit})
	if err != nil {
		if errors.Is(err, frost.ErrNotConfigured) {
			c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Frost client credentials are not configured"))
			return
		}
		if errors.Is(err, frost.ErrInvalidRequest) {
			c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "Frost request is outside the allowed bounds"))
			return
		}
		c.JSON(http.StatusBadGateway, errorPayload("frost_unavailable", "Frost observations are unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) AvalancheWarnings(c *gin.Context) {
	if h.sources.Geospatial == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "NVE warnings are not configured"))
		return
	}
	lat, lon, ok := requiredCoordinates(c)
	if !ok {
		return
	}
	startDate := strings.TrimSpace(c.Query("startDate"))
	endDate := strings.TrimSpace(c.Query("endDate"))
	if startDate == "" || endDate == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_param", "startDate and endDate are required"))
		return
	}
	language := strings.TrimSpace(c.Query("language"))
	if language == "" {
		language = "no"
	}
	payload, err := h.sources.Geospatial.AvalancheWarnings(c.Request.Context(), lat, lon, language, startDate, endDate)
	if err != nil {
		h.writeSourceError(c, err, geospatial.ErrInvalidRequest, "nve_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) HeritageFeatures(c *gin.Context) {
	if h.sources.Geospatial == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Cultural heritage data is not configured"))
		return
	}
	minLon, minLat, maxLon, maxLat, ok := requiredBBox(c)
	if !ok {
		return
	}
	limit, err := parseQueryInt(c, "limit", 50)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "limit must be an integer"))
		return
	}
	payload, err := h.sources.Geospatial.HeritageFeatures(c.Request.Context(), minLon, minLat, maxLon, maxLat, limit)
	if err != nil {
		h.writeSourceError(c, err, geospatial.ErrInvalidRequest, "heritage_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) AggregateAirQuality(c *gin.Context) {
	if h.sources.Geospatial == nil {
		c.JSON(http.StatusServiceUnavailable, errorPayload("source_not_configured", "Aggregated air quality is not configured"))
		return
	}
	meanType, err := parseQueryInt(c, "meanType", 0)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "meanType must be an integer"))
		return
	}
	lat, lon, ok := requiredCoordinates(c)
	if !ok {
		return
	}
	radius, err := parseQueryFloat(c, "radius")
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "radius must be a number"))
		return
	}
	fromTime := strings.TrimSpace(c.Query("from"))
	toTime := strings.TrimSpace(c.Query("to"))
	if fromTime == "" || toTime == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_param", "from and to are required"))
		return
	}
	payload, err := h.sources.Geospatial.AggregateAirQuality(c.Request.Context(), meanType, fromTime, toTime, lat, lon, radius, c.Query("method"))
	if err != nil {
		h.writeSourceError(c, err, geospatial.ErrInvalidRequest, "air_quality_unavailable")
		return
	}
	c.JSON(http.StatusOK, payload)
}

func (h *Handler) writeSourceError(c *gin.Context, err, invalid error, code string) {
	if errors.Is(err, invalid) {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "request is outside the allowed bounds"))
		return
	}
	c.JSON(http.StatusBadGateway, errorPayload(code, "Source data is unavailable right now."))
}

func decodeJSON(c *gin.Context, target any) bool {
	c.Request.Body = http.MaxBytesReader(c.Writer, c.Request.Body, 32<<10)
	if err := json.NewDecoder(c.Request.Body).Decode(target); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "request body must be valid JSON"))
		return false
	}
	return true
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

func (h *Handler) WeatherOslo(c *gin.Context) {
	payload, err := h.weather.Oslo(c.Request.Context(), h.cfg.UserAgent)
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("weather_unavailable", "Weather data is unavailable right now."))
		return
	}
	c.JSON(http.StatusOK, payload)
}

func parseQueryInt(c *gin.Context, key string, fallback int) (int, error) {
	raw := c.Query(key)
	if raw == "" {
		return fallback, nil
	}
	value, err := strconv.Atoi(raw)
	if err != nil {
		return 0, err
	}
	return value, nil
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

func parseQueryFloat(c *gin.Context, key string) (float64, error) {
	raw := strings.TrimSpace(c.Query(key))
	if raw == "" {
		return 0, errors.New("missing query parameter")
	}
	return strconv.ParseFloat(raw, 64)
}

func requiredCoordinates(c *gin.Context) (float64, float64, bool) {
	lat, err := parseQueryFloat(c, "lat")
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "lat must be a number"))
		return 0, 0, false
	}
	lon, err := parseQueryFloat(c, "lon")
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_param", "lon must be a number"))
		return 0, 0, false
	}
	return lat, lon, true
}

func requiredBBox(c *gin.Context) (float64, float64, float64, float64, bool) {
	keys := []string{"minLon", "minLat", "maxLon", "maxLat"}
	values := make([]float64, len(keys))
	for i, key := range keys {
		value, err := parseQueryFloat(c, key)
		if err != nil {
			c.JSON(http.StatusBadRequest, errorPayload("invalid_param", key+" must be a number"))
			return 0, 0, 0, 0, false
		}
		values[i] = value
	}
	return values[0], values[1], values[2], values[3], true
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
