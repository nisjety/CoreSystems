package api

import (
	"context"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/asyncjobs"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/sse"
)

// llmsTxtAsyncRequest is the request body for POST /v1/llmstxt (async create).
type llmsTxtAsyncRequest struct {
	URL        string                `json:"url"`
	Full       bool                  `json:"full,omitempty"`
	TimeoutSec int                   `json:"timeout,omitempty"`
	Webhook    *models.WebhookConfig `json:"webhook,omitempty"`
}

// llmsTxtStatusResponse is the polling response for GET /v1/llmstxt/:id.
type llmsTxtStatusResponse struct {
	Success   bool   `json:"success"`
	ID        string `json:"id"`
	Status    string `json:"status"`
	URL       string `json:"url,omitempty"`
	Full      bool   `json:"full,omitempty"`
	CreatedAt string `json:"createdAt,omitempty"`
	ExpiresAt string `json:"expiresAt,omitempty"`
	Error     string `json:"error,omitempty"`
	Data      *llmsTxtResultData `json:"data,omitempty"`
}

type llmsTxtResultData struct {
	Markdown string `json:"markdown"`
}

// v1LlmsTxtCreate handles POST /v1/llmstxt — enqueues an async LLMs.txt job.
// The legacy GET /v1/llmstxt (sync) is preserved unchanged.
func (h *Handler) v1LlmsTxtCreate(c *fiber.Ctx) error {
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	var req llmsTxtAsyncRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}

	req.URL = strings.TrimSpace(req.URL)
	if req.URL == "" {
		return writeError(c, http.StatusBadRequest, "url is required", nil)
	}
	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if req.Webhook != nil && strings.TrimSpace(req.Webhook.URL) != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}

	timeoutSec := req.TimeoutSec
	if timeoutSec <= 0 {
		timeoutSec = 300 // 5 minutes — generous for large sites
	}

	orgID, userID, _ := h.currentOrgAndUser(c)

	job := h.jobStore.New(map[string]string{
		"kind":        "llmstxt",
		"api_version": "v1",
		"url":         h.redactStoredString(c, req.URL),
		"org_id":      orgID,
		"user_id":     userID,
	})
	_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
		current.Result = map[string]any{
			"url":  req.URL,
			"full": req.Full,
		}
	})

	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{
			"jobId":    job.ID,
			"resource": "llmstxt",
			"url":      req.URL,
		})
	}

	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindLlmsTxt, job.ID, asyncjobs.LlmsTxtPayload{
		OrgID:      orgID,
		UserID:     userID,
		URL:        req.URL,
		Full:       req.Full,
		Webhook:    req.Webhook,
		TimeoutSec: timeoutSec,
	}, "v1"); err != nil {
		markJobDispatchFailure(h.jobStore, job.ID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue llmstxt job", err.Error())
	}

	h.recordUserActivity(c, "llmstxt.created", "llmstxt", map[string]interface{}{
		"jobId":   job.ID,
		"summary": "llmstxt queued",
		"url":     h.redactStoredString(c, req.URL),
	})

	return c.JSON(h.newAsyncCreateEnvelope(c, "llmstxt", job.ID, job.CreatedAt, job.ExpiresAt, "queued", nil))
}

// v1LlmsTxtStatus handles GET /v1/llmstxt/:id — polls an async LLMs.txt job.
func (h *Handler) v1LlmsTxtStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "llmstxt" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	resp := llmsTxtStatusResponse{
		Success:   job.Status != jobs.StatusFailed,
		ID:        job.ID,
		Status:    string(job.Status),
		CreatedAt: job.CreatedAt.Format(time.RFC3339),
		ExpiresAt: job.ExpiresAt.Format(time.RFC3339),
		Error:     job.Error,
	}
	if u, ok := job.Result["url"].(string); ok {
		resp.URL = u
	}
	if f, ok := job.Result["full"].(bool); ok {
		resp.Full = f
	}
	if md, ok := job.Result["markdown"].(string); ok && md != "" {
		resp.Data = &llmsTxtResultData{Markdown: md}
	}

	return c.JSON(resp)
}

// runLlmsTxtJob is the async runner invoked by HandleAsyncJob.
func (h *Handler) runLlmsTxtJob(ctx context.Context, jobID string, payload asyncjobs.LlmsTxtPayload) {
	timeout := time.Duration(payload.TimeoutSec) * time.Second
	if timeout <= 0 {
		timeout = 300 * time.Second
	}
	runCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobStarted, map[string]any{"progress": 0})
	}

	markdown, err := h.generateLLMsTxt(runCtx, payload.URL, payload.Full)
	if err != nil {
		_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = err.Error()
		})
		if h.streamManager != nil {
			h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"error": err.Error()})
		}
		if payload.Webhook != nil && strings.TrimSpace(payload.Webhook.URL) != "" {
			go h.fireWebhook(payload.Webhook.URL, &models.WebhookPayload{
				Success:  false,
				Type:     "llmstxt.failed",
				Metadata: map[string]interface{}{"jobId": jobID, "error": err.Error()},
			})
		}
		zlog.Error().Err(err).Str("job_id", jobID).Msg("llmstxt job failed")
		return
	}

	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusReady
		current.Progress = 100
		current.Result["markdown"] = markdown
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobCompleted, map[string]any{"progress": 100})
	}

	if payload.Webhook != nil && strings.TrimSpace(payload.Webhook.URL) != "" {
		go h.fireWebhook(payload.Webhook.URL, &models.WebhookPayload{
			Success: true,
			Type:    "llmstxt.completed",
			Metadata: map[string]interface{}{
				"jobId":    jobID,
				"url":      payload.URL,
				"full":     payload.Full,
				"markdown": markdown,
			},
		})
	}

	zlog.Info().Str("job_id", jobID).Str("url", payload.URL).Msg("llmstxt job completed")
}
