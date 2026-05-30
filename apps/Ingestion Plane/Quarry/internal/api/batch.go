package api

import (
	"context"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/models"
)

func (h *Handler) batchCrawl(c *fiber.Ctx) error {
	if h.batchManager == nil {
		return writeError(c, http.StatusServiceUnavailable, "batch manager is not initialized", nil)
	}

	var req models.BatchScrapeRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	if len(req.URLs) == 0 {
		return writeError(c, http.StatusBadRequest, "urls is required", nil)
	}
	if len(req.URLs) > defaultBatchURLsLimit {
		return writeError(c, http.StatusBadRequest, "urls exceeds configured batch limit", nil)
	}
	for _, u := range req.URLs {
		if err := validateAbsoluteHTTPURL(u); err != nil {
			return writeError(c, http.StatusBadRequest, "urls contains invalid value", err.Error())
		}
	}
	if req.MaxAge < 0 {
		return writeError(c, http.StatusBadRequest, "maxAge must be >= 0", nil)
	}
	if req.WaitTimeout < 0 || req.WaitTimeout > defaultBatchWaitTimeout {
		return writeError(c, http.StatusBadRequest, "waitTimeout is out of allowed range", nil)
	}
	if req.PollInterval < 0 || req.PollInterval > 30 {
		return writeError(c, http.StatusBadRequest, "pollInterval is out of allowed range", nil)
	}
	if req.Webhook != nil {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
		for _, event := range req.Webhook.Events {
			switch strings.TrimSpace(event) {
			case "", "started", "page", "completed", "failed":
			default:
				return writeError(c, http.StatusBadRequest, "webhook events contains unsupported value", event)
			}
		}
	}

	job := h.batchManager.CreateJob(&req)
	jobCtx := context.Background()
	if req.WaitTimeout > 0 {
		jobCtx = c.UserContext()
	}
	if err := h.batchManager.StartJob(jobCtx, job.ID); err != nil {
		return writeError(c, http.StatusBadGateway, "failed to start batch job", err.Error())
	}

	if req.WaitTimeout > 0 {
		poll := time.Duration(req.PollInterval) * time.Second
		if poll <= 0 {
			poll = 500 * time.Millisecond
		}
		status, waitErr := h.batchManager.WaitForCompletion(c.UserContext(), job.ID, time.Duration(req.WaitTimeout)*time.Second, poll)
		if waitErr != nil {
			return c.Status(http.StatusAccepted).JSON(fiber.Map{
				"success": true,
				"id":      job.ID,
				"status":  "processing",
			})
		}
		return c.JSON(fiber.Map{"success": true, "id": job.ID, "status": status})
	}

	return c.Status(http.StatusAccepted).JSON(models.BatchScrapeStartResponse{
		Success: true,
		ID:      job.ID,
		URL:     "/v1/batch/" + job.ID,
	})
}

func (h *Handler) batchStatus(c *fiber.Ctx) error {
	if h.batchManager == nil {
		return writeError(c, http.StatusServiceUnavailable, "batch manager is not initialized", nil)
	}

	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "batch id is required", nil)
	}

	status := h.batchManager.GetJobStatus(jobID)
	if status == nil {
		return writeError(c, http.StatusNotFound, "batch job not found", nil)
	}

	return c.JSON(fiber.Map{"success": true, "id": jobID, "status": status})
}
