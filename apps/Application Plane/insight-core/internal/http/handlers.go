package http

import (
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
	"github.com/gin-gonic/gin"
)

type Handler struct {
	cfg     *config.Config
	service *insights.Service
}

func NewHandler(cfg *config.Config, service *insights.Service) *Handler {
	return &Handler{cfg: cfg, service: service}
}

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"status": "ok", "service": h.cfg.ServiceName})
}

func (h *Handler) Overview(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	query, err := overviewQueryFromRequest(c, orgID)
	if err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_query", err.Error()))
		return
	}
	overview, err := h.service.Overview(c.Request.Context(), query)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": overview})
}

func (h *Handler) ListConnectors(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	connectors, err := h.service.ListConnectorSlots(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": connectors})
}

func (h *Handler) IngestEvent(c *gin.Context) {
	var body insights.IngestMetricEventInput
	if err := decodeJSONBody(c, &body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	event, err := h.service.RecordMetricEvent(c.Request.Context(), body)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusAccepted, gin.H{"data": event})
}

func overviewQueryFromRequest(c *gin.Context, orgID string) (insights.OverviewQuery, error) {
	from, err := optionalTime(c.Query("from"))
	if err != nil {
		return insights.OverviewQuery{}, err
	}
	to, err := optionalTime(c.Query("to"))
	if err != nil {
		return insights.OverviewQuery{}, err
	}
	return insights.OverviewQuery{
		OrgID:    orgID,
		Surfaces: surfacesFromRequest(c),
		From:     from,
		To:       to,
	}, nil
}

func surfacesFromRequest(c *gin.Context) []string {
	values := append([]string{}, c.QueryArray("surface")...)
	if raw := strings.TrimSpace(c.Query("surfaces")); raw != "" {
		values = append(values, strings.Split(raw, ",")...)
	}
	return values
}

func optionalTime(value string) (*time.Time, error) {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil, nil
	}
	if parsed, err := time.Parse(time.RFC3339, value); err == nil {
		utc := parsed.UTC()
		return &utc, nil
	}
	if parsed, err := time.Parse("2006-01-02", value); err == nil {
		utc := parsed.UTC()
		return &utc, nil
	}
	return nil, errors.New("time values must be RFC3339 timestamps or YYYY-MM-DD dates")
}

func decodeJSONBody(c *gin.Context, target any) error {
	decoder := json.NewDecoder(c.Request.Body)
	decoder.DisallowUnknownFields()
	return decoder.Decode(target)
}

func requireOrgID(c *gin.Context) string {
	orgID := strings.TrimSpace(c.GetHeader("x-org-id"))
	if orgID == "" {
		orgID = strings.TrimSpace(c.Query("org_id"))
	}
	if orgID == "" {
		orgID = strings.TrimSpace(c.Query("orgId"))
	}
	if orgID == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_org_id", "x-org-id is required."))
		return ""
	}
	return orgID
}

func writeServiceError(c *gin.Context, err error) {
	switch {
	case insights.IsInvalidInput(err):
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", err.Error()))
	default:
		c.JSON(http.StatusInternalServerError, errorPayload("internal_error", "Internal error."))
	}
}

func errorPayload(code, message string) gin.H {
	return gin.H{"error": gin.H{"code": code, "message": message}}
}
