package http

import (
	"errors"
	"fmt"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/brreg"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/leads"
)

type Handler struct {
	cfg     *config.Config
	service *leads.Service
}

func NewHandler(cfg *config.Config, service *leads.Service) *Handler {
	return &Handler{cfg: cfg, service: service}
}

func errorPayload(code, message string) gin.H {
	return gin.H{"error": gin.H{"code": code, "message": message}}
}

func requireOrgID(c *gin.Context) string {
	orgID := strings.TrimSpace(c.GetHeader("x-org-id"))
	if orgID == "" {
		c.JSON(http.StatusBadRequest, errorPayload("missing_org_id", "x-org-id is required."))
		return ""
	}
	return orgID
}

func actorUserID(c *gin.Context) string {
	if v := strings.TrimSpace(c.GetHeader("x-user-id")); v != "" {
		return v
	}
	return "internal-service"
}

func (h *Handler) Health(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"status": "ok", "service": h.cfg.ServiceName})
}

type searchBody struct {
	Naeringskode         string `json:"naeringskode"`
	Kommunenummer        string `json:"kommunenummer"`
	Organisasjonsform    string `json:"organisasjonsform"`
	FraAntallAnsatte     *int   `json:"fra_antall_ansatte"`
	TilAntallAnsatte     *int   `json:"til_antall_ansatte"`
	FraRegistreringsdato string `json:"fra_registreringsdato"`
	TilRegistreringsdato string `json:"til_registreringsdato"`
	Page                 int    `json:"page"`
	Size                 int    `json:"size"`
}

func (h *Handler) Search(c *gin.Context) {
	if requireOrgID(c) == "" {
		return
	}
	var body searchBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	page, err := h.service.Search(c.Request.Context(), brreg.SearchFilter{
		Naeringskode:         body.Naeringskode,
		Kommunenummer:        body.Kommunenummer,
		Organisasjonsform:    body.Organisasjonsform,
		FraAntallAnsatte:     body.FraAntallAnsatte,
		TilAntallAnsatte:     body.TilAntallAnsatte,
		FraRegistreringsdato: body.FraRegistreringsdato,
		TilRegistreringsdato: body.TilRegistreringsdato,
		Page:                 body.Page,
		Size:                 body.Size,
	})
	if err != nil {
		// Unsatisfiable-filter errors are client errors (422), not 500s.
		if errors.Is(err, brreg.ErrEmployeeBandUnsupported) || errors.Is(err, brreg.ErrDeepPagingLimit) {
			c.JSON(http.StatusUnprocessableEntity, errorPayload("invalid_filter", err.Error()))
			return
		}
		c.JSON(http.StatusBadGateway, errorPayload("brreg_error", "Enhetsregisteret search failed."))
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": page})
}

// Branches returns a company's sub-entities/branches (/underenheter). Org is
// resolved server-side from x-org-id (set by the gateway), never from the body.
func (h *Handler) Branches(c *gin.Context) {
	if requireOrgID(c) == "" {
		return
	}
	branches, err := h.service.Branches(c.Request.Context(), c.Param("orgnr"))
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("brreg_error", "Enhetsregisteret sub-entity lookup failed."))
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": branches, "meta": gin.H{"count": len(branches)}})
}

// Financials returns a company's filed annual accounts (/regnskap). Aggregate
// company figures only. Org is resolved server-side from x-org-id.
func (h *Handler) Financials(c *gin.Context) {
	if requireOrgID(c) == "" {
		return
	}
	fins, err := h.service.Financials(c.Request.Context(), c.Param("orgnr"))
	if err != nil {
		c.JSON(http.StatusBadGateway, errorPayload("brreg_error", "Regnskapsregisteret lookup failed."))
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": fins, "meta": gin.H{"count": len(fins)}})
}

type buildListBody struct {
	Name            string `json:"name"`
	IncludeBranches bool   `json:"include_branches"`
	Filter          struct {
		Naeringskode         string `json:"naeringskode"`
		Kommunenummer        string `json:"kommunenummer"`
		Organisasjonsform    string `json:"organisasjonsform"`
		FraAntallAnsatte     *int   `json:"fra_antall_ansatte"`
		TilAntallAnsatte     *int   `json:"til_antall_ansatte"`
		FraRegistreringsdato string `json:"fra_registreringsdato"`
		TilRegistreringsdato string `json:"til_registreringsdato"`
		Page                 int    `json:"page"`
		Size                 int    `json:"size"`
	} `json:"filter"`
}

// BuildList is the governed `leads.build_list` action surface: search → (branch
// enrich) → dedupe → save an org-scoped list. The org is resolved server-side
// from x-org-id (set by the gateway after authorizing the session); it is never
// read from the request body, so the action is IDOR-clean.
func (h *Handler) BuildList(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body buildListBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	list, err := h.service.BuildList(c.Request.Context(), leads.BuildListInput{
		OrgID:           orgID,
		Name:            body.Name,
		CreatedBy:       actorUserID(c),
		IncludeBranches: body.IncludeBranches,
		Filter: brreg.SearchFilter{
			Naeringskode:         body.Filter.Naeringskode,
			Kommunenummer:        body.Filter.Kommunenummer,
			Organisasjonsform:    body.Filter.Organisasjonsform,
			FraAntallAnsatte:     body.Filter.FraAntallAnsatte,
			TilAntallAnsatte:     body.Filter.TilAntallAnsatte,
			FraRegistreringsdato: body.Filter.FraRegistreringsdato,
			TilRegistreringsdato: body.Filter.TilRegistreringsdato,
			Page:                 body.Filter.Page,
			Size:                 body.Filter.Size,
		},
	})
	if err != nil {
		if errors.Is(err, brreg.ErrEmployeeBandUnsupported) || errors.Is(err, brreg.ErrDeepPagingLimit) {
			c.JSON(http.StatusUnprocessableEntity, errorPayload("invalid_filter", err.Error()))
			return
		}
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": list})
}

type createListBody struct {
	Name      string          `json:"name"`
	Companies []brreg.Company `json:"companies"`
}

func (h *Handler) CreateList(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	var body createListBody
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, errorPayload("invalid_json", "Request body is invalid."))
		return
	}
	list, err := h.service.CreateList(c.Request.Context(), leads.CreateListInput{
		OrgID:     orgID,
		Name:      body.Name,
		CreatedBy: actorUserID(c),
		Companies: body.Companies,
	})
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": list})
}

func (h *Handler) ListLists(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	lists, err := h.service.ListLists(c.Request.Context(), orgID)
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": lists, "meta": gin.H{"count": len(lists)}})
}

func (h *Handler) GetList(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	list, err := h.service.GetList(c.Request.Context(), orgID, c.Param("id"))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": list})
}

func (h *Handler) DeleteList(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	if err := h.service.DeleteList(c.Request.Context(), orgID, c.Param("id")); err != nil {
		writeServiceError(c, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"ok": true}})
}

func (h *Handler) ExportCSV(c *gin.Context) {
	orgID := requireOrgID(c)
	if orgID == "" {
		return
	}
	listID := c.Param("id")
	csvBytes, list, err := h.service.ExportCSV(c.Request.Context(), orgID, listID, actorUserID(c))
	if err != nil {
		writeServiceError(c, err)
		return
	}
	filename := fmt.Sprintf("leads-%s.csv", list.ID)
	c.Header("Content-Disposition", fmt.Sprintf("attachment; filename=%q", filename))
	// Surface the count so the gateway can record it on the per-export audit event.
	c.Header("X-Lead-Count", fmt.Sprintf("%d", list.CompanyCount))
	c.Data(http.StatusOK, "text/csv; charset=utf-8", csvBytes)
}

func writeServiceError(c *gin.Context, err error) {
	switch {
	case errors.Is(err, leads.ErrNotFound):
		c.JSON(http.StatusNotFound, errorPayload("not_found", "Lead list not found."))
	case errors.Is(err, leads.ErrInvalidInput):
		c.JSON(http.StatusUnprocessableEntity, errorPayload("validation_error", err.Error()))
	default:
		c.JSON(http.StatusInternalServerError, errorPayload("internal_error", "Internal error."))
	}
}
