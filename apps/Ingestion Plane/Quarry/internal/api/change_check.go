package api

import (
	"net/http"
	"strings"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
)

type changeCheckRequest struct {
	URL      string                 `json:"url"`
	Tag      string                 `json:"tag,omitempty"`
	Modes    []string               `json:"modes,omitempty"`
	DryRun   bool                   `json:"dryRun,omitempty"`
	Headers  map[string]string      `json:"headers,omitempty"`
	WaitFor  int                    `json:"waitFor,omitempty"`
	OnlyMain bool                   `json:"onlyMainContent,omitempty"`
	Actions  []models.ActionRequest `json:"actions,omitempty"`
}

func (h *Handler) checkChangeNow(c *fiber.Ctx) error {
	if h.changeTracker == nil {
		return writeError(c, http.StatusServiceUnavailable, "change tracker is not initialized", nil)
	}
	if h.scraper == nil {
		return writeError(c, http.StatusServiceUnavailable, "scraper is not initialized", nil)
	}

	var req changeCheckRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}

	req.URL = strings.TrimSpace(req.URL)
	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}

	formatOpts := &scraper.FormatOptions{
		Formats:         []string{"markdown"},
		Headers:         req.Headers,
		WaitFor:         req.WaitFor,
		OnlyMainContent: req.OnlyMain,
		Actions:         toActionSteps(req.Actions),
	}
	outputs, _, err := h.scraper.FetchFormats(c.UserContext(), req.URL, formatOpts)
	if err != nil {
		return writeError(c, http.StatusBadGateway, "failed to fetch current content", err.Error())
	}

	currentPayload := ""
	if markdown, ok := outputs["markdown"].(string); ok {
		currentPayload = strings.TrimSpace(markdown)
	}
	if currentPayload == "" {
		return writeError(c, http.StatusBadGateway, "failed to build change tracking payload", nil)
	}

	trackReq := &models.ChangeTrackingRequest{
		Enabled: true,
		Tag:     strings.TrimSpace(req.Tag),
		Modes:   req.Modes,
	}

	var (
		result   *models.ChangeTrackingResult
		trackErr error
	)
	if req.DryRun {
		result, trackErr = h.changeTracker.Compare(c.UserContext(), req.URL, currentPayload, trackReq)
	} else {
		result, trackErr = h.changeTracker.Track(c.UserContext(), req.URL, currentPayload, trackReq)
	}
	if trackErr != nil {
		return writeError(c, http.StatusBadGateway, "change check failed", trackErr.Error())
	}

	return c.JSON(fiber.Map{
		"success":        true,
		"url":            req.URL,
		"tag":            trackReq.Tag,
		"dryRun":         req.DryRun,
		"changeTracking": result,
	})
}
