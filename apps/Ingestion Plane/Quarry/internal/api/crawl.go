package api

import (
	"context"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/executor"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/pipeline"
	"github.com/triodelab/quarry/internal/sse"
)

func (h *Handler) crawl(c *fiber.Ctx) error {
	if h.jobStore == nil || h.execRouter == nil || h.immediateExec == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl executor is not initialized", nil)
	}

	var req models.CrawlAPIRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}

	if strings.TrimSpace(req.URL) == "" {
		return writeError(c, http.StatusBadRequest, "url is required", nil)
	}
	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if req.MaxDepth <= 0 {
		req.MaxDepth = 1
	}
	if req.MaxDepth > 20 {
		return writeError(c, http.StatusBadRequest, "maxDepth exceeds configured limit", nil)
	}
	if req.Module != "" && !modulePattern.MatchString(req.Module) {
		return writeError(c, http.StatusBadRequest, "module contains invalid characters", nil)
	}
	if h.cfg != nil {
		if req.EnrichLimit < 0 || req.EnrichLimit > h.cfg.ScrapeMaxEnrichLimit {
			return writeError(c, http.StatusBadRequest, "enrichLimit is out of allowed range", nil)
		}
		if req.MaxAge < 0 || req.MaxAge > h.cfg.ScrapeMaxAgeMs {
			return writeError(c, http.StatusBadRequest, "maxAge is out of allowed range", nil)
		}
	}

	selectedMode := h.execRouter.Select(&req)
	jobStart := time.Now()
	meta := map[string]string{
		"url":    req.URL,
		"mode":   string(selectedMode),
		"module": req.Module,
	}
	job := h.jobStore.New(meta)
	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{"jobId": job.ID, "mode": selectedMode})
	}

	// Publish crawl started event to shared NATS
	if h.sharedPublisher != nil {
		// Convert meta to interface{} map for NATS publisher
		metaIface := make(map[string]interface{})
		for k, v := range meta {
			metaIface[k] = v
		}
		_ = h.sharedPublisher.PublishCrawlStarted(
			c.UserContext(),
			c.Get("X-Org-ID"), // Extract org_id from request context
			req.URL,
			job.ID,
			metaIface,
		)
	}

	// Wire page-level SSE events into the crawl request
	if h.streamManager != nil {
		req.EventSink = &ssePageEventSink{sm: h.streamManager, jobID: job.ID}
	}

	switch selectedMode {
	case executor.ModeImmediate:
		_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusRunning
			current.Progress = 50
		})
		if h.streamManager != nil {
			h.streamManager.Broadcast(job.ID, sse.EventJobStarted, map[string]any{"progress": 50})
		}

		result, err := h.immediateExec.Execute(c.UserContext(), &req)
		if err != nil {
			updated, _ := h.jobStore.Update(job.ID, func(current *jobs.Job) {
				current.Status = jobs.StatusFailed
				current.Error = err.Error()
			})
			if h.streamManager != nil {
				h.streamManager.Broadcast(job.ID, sse.EventJobFailed, map[string]any{"error": err.Error()})
			}

			// Publish crawl failed event to shared NATS
			if h.sharedPublisher != nil {
				jobMetaIface := make(map[string]interface{})
				for k, v := range job.Meta {
					jobMetaIface[k] = v
				}
				_ = h.sharedPublisher.PublishCrawlFailed(
					c.UserContext(),
					c.Get("X-Org-ID"),
					req.URL,
					job.ID,
					err.Error(),
					jobMetaIface,
				)
			}

			// Fire webhook on failure
			if req.Webhook != nil && req.Webhook.URL != "" {
				go h.fireWebhook(req.Webhook.URL, &models.WebhookPayload{
					Success: false,
					Type:    "crawl.failed",
					ID:      job.ID,
					Error:   err.Error(),
				})
			}
			if updated == nil {
				updated = job
			}
			return c.Status(http.StatusBadGateway).JSON(models.CrawlAPIResponse{
				Success: false,
				Job:     toJobSummary(updated),
				Error:   err.Error(),
			})
		}

		updated, _ := h.jobStore.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusReady
			current.Progress = 100
			current.Result = map[string]any{"data": result}
		})
		if updated == nil {
			updated = job
		}

		if h.pipelineChain != nil {
			payload := map[string]any{}
			for key, value := range updated.Result {
				payload[key] = value
			}
			pipelineCtx := buildPipelineContext(updated.ID, req.URL, string(executor.ModeImmediate), req.Module, jobStart, payload, updated.Meta)
			if err := h.pipelineChain.Run(c.UserContext(), pipelineCtx); err == nil {
				updated, _ = h.jobStore.Update(updated.ID, func(current *jobs.Job) {
					current.Result = pipelineCtx.Result
					current.Meta = pipelineCtx.Meta
				})
				if updated == nil {
					updated = job
				}
			}
		}
		if h.streamManager != nil {
			h.streamManager.Broadcast(updated.ID, sse.EventJobCompleted, map[string]any{"progress": 100})
		}

		// Publish crawl completed event to shared NATS (JetStream, cross-plane)
		if h.sharedPublisher != nil {
			updatedMetaIface := make(map[string]interface{})
			for k, v := range updated.Meta {
				updatedMetaIface[k] = v
			}
			pageCount := 0
			_ = h.sharedPublisher.PublishCrawlCompleted(
				c.UserContext(),
				c.Get("X-Org-ID"),
				req.URL,
				updated.ID,
				pageCount,
				updatedMetaIface,
			)

			// Notify the requesting user via notification-core (plain NATS)
			if userID := c.Get("X-User-ID"); userID != "" {
				_ = h.sharedPublisher.PublishNotificationCrawlCompleted(
					c.UserContext(),
					userID,
					updated.ID,
					req.URL,
					c.Get("X-Org-ID"),
					pageCount,
				)
			}
		}

		// Fire webhook on success
		if req.Webhook != nil && req.Webhook.URL != "" {
			go h.fireWebhook(req.Webhook.URL, &models.WebhookPayload{
				Success: true,
				Type:    "crawl.completed",
				ID:      updated.ID,
				Data:    nil, // could add serialised result here if needed
			})
		}

		return c.JSON(models.CrawlAPIResponse{
			Success: true,
			Job:     toJobSummary(updated),
			Dispatch: &models.CrawlDispatch{
				Mode: string(executor.ModeImmediate),
			},
			Data: result,
		})

	case executor.ModeScheduled:
		if h.scheduledExec == nil || !h.scheduledExec.Enabled() {
			_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
				current.Status = jobs.StatusFailed
				current.Error = "scheduled execution is disabled"
			})
			if h.streamManager != nil {
				h.streamManager.Broadcast(job.ID, sse.EventJobFailed, map[string]any{"error": "scheduled execution is disabled"})
			}

			// Publish crawl failed event to shared NATS
			if h.sharedPublisher != nil {
				jobMetaIface := make(map[string]interface{})
				for k, v := range job.Meta {
					jobMetaIface[k] = v
				}
				_ = h.sharedPublisher.PublishCrawlFailed(
					c.UserContext(),
					c.Get("X-Org-ID"),
					req.URL,
					job.ID,
					"scheduled execution is disabled",
					jobMetaIface,
				)
			}

			return writeError(c, http.StatusServiceUnavailable, "scheduled execution is disabled", nil)
		}

		dispatch, err := h.scheduledExec.DispatchCrawl(c.UserContext(), job.ID, &req)
		if err != nil {
			_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
				current.Status = jobs.StatusFailed
				current.Error = err.Error()
			})
			if h.streamManager != nil {
				h.streamManager.Broadcast(job.ID, sse.EventJobFailed, map[string]any{"error": err.Error()})
			}

			// Publish crawl failed event to shared NATS
			if h.sharedPublisher != nil {
				jobMetaIface := make(map[string]interface{})
				for k, v := range job.Meta {
					jobMetaIface[k] = v
				}
				_ = h.sharedPublisher.PublishCrawlFailed(
					c.UserContext(),
					c.Get("X-Org-ID"),
					req.URL,
					job.ID,
					err.Error(),
					jobMetaIface,
				)
			}

			return c.Status(http.StatusBadGateway).JSON(models.CrawlAPIResponse{
				Success: false,
				Job:     toJobSummary(job),
				Error:   err.Error(),
			})
		}

		updated, _ := h.jobStore.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusRunning
			current.Progress = 10
			if current.Meta == nil {
				current.Meta = map[string]string{}
			}
			current.Meta["workflowId"] = dispatch.WorkflowID
			current.Meta["runId"] = dispatch.RunID
		})
		if updated == nil {
			updated = job
		}
		if h.streamManager != nil {
			h.streamManager.Broadcast(updated.ID, sse.EventJobStarted, map[string]any{"progress": 10, "workflowId": dispatch.WorkflowID, "runId": dispatch.RunID})
		}

		return c.Status(http.StatusAccepted).JSON(models.CrawlAPIResponse{
			Success: true,
			Job:     toJobSummary(updated),
			Dispatch: &models.CrawlDispatch{
				Mode:       string(executor.ModeScheduled),
				WorkflowID: dispatch.WorkflowID,
				RunID:      dispatch.RunID,
			},
		})

	default:
		return writeError(c, http.StatusBadRequest, "unsupported execution mode", nil)
	}
}

func (h *Handler) getJobStatus(c *fiber.Ctx) error {
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok {
		if h.scheduledExec != nil && h.scheduledExec.Enabled() {
			ctx, cancel := context.WithTimeout(c.UserContext(), 3*time.Second)
			defer cancel()
			if hydrated, err := h.scheduledExec.RehydrateJob(ctx, h.jobStore, jobID); err == nil && hydrated != nil {
				job = hydrated
				ok = true
			}
		}
		if !ok {
			return writeError(c, http.StatusNotFound, "job not found", nil)
		}
	}

	if h.scheduledExec != nil && h.scheduledExec.Enabled() {
		ctx, cancel := context.WithTimeout(c.UserContext(), 3*time.Second)
		defer cancel()
		if refreshed, err := h.scheduledExec.RefreshJob(ctx, h.jobStore, jobID); err == nil && refreshed != nil {
			job = refreshed
			if job.Status == jobs.StatusReady && h.pipelineChain != nil {
				payload := map[string]any{}
				for key, value := range job.Result {
					payload[key] = value
				}
				pipelineCtx := buildPipelineContext(job.ID, job.Meta["url"], job.Meta["mode"], job.Meta["module"], job.CreatedAt, payload, job.Meta)
				if runErr := h.pipelineChain.Run(c.UserContext(), pipelineCtx); runErr == nil {
					updated, _ := h.jobStore.Update(job.ID, func(current *jobs.Job) {
						current.Result = pipelineCtx.Result
						current.Meta = pipelineCtx.Meta
					})
					if updated != nil {
						job = updated
					}
				}
			}
		}
	}

	return c.JSON(models.JobStatusResponse{
		Success: true,
		Job:     toJobSummary(job),
		Result:  job.Result,
		Error:   job.Error,
	})
}

func toJobSummary(job *jobs.Job) *models.JobSummary {
	if job == nil {
		return nil
	}
	return &models.JobSummary{
		ID:        job.ID,
		Status:    string(job.Status),
		CreatedAt: job.CreatedAt,
		UpdatedAt: job.UpdatedAt,
	}
}

func buildPipelineContext(jobID, targetURL, mode, module string, startedAt time.Time, result map[string]any, meta map[string]string) *pipeline.JobContext {
	ctxMeta := map[string]string{}
	for key, value := range meta {
		ctxMeta[key] = value
	}
	ctxResult := map[string]any{}
	for key, value := range result {
		ctxResult[key] = value
	}

	return &pipeline.JobContext{
		JobID:     jobID,
		URL:       targetURL,
		Mode:      mode,
		Module:    module,
		StartedAt: startedAt,
		Result:    ctxResult,
		Meta:      ctxMeta,
	}
}

// ssePageEventSink adapts sse.StreamManager to the models.PageEventSink interface
// so the scraper can broadcast page-level events without importing the sse package.
type ssePageEventSink struct {
	sm    *sse.StreamManager
	jobID string
}

func (s *ssePageEventSink) OnPageDiscovered(url string, count int) {
	s.sm.Broadcast(s.jobID, sse.EventPageDiscovered, map[string]any{
		"url":   url,
		"count": count,
	})
}

func (s *ssePageEventSink) OnPageClassified(url string, pageType string, confidence float64) {
	s.sm.Broadcast(s.jobID, sse.EventPageClassified, map[string]any{
		"url":        url,
		"page_type":  pageType,
		"confidence": confidence,
	})
}

func (s *ssePageEventSink) OnPageEnriched(url string, pageType string, title string) {
	s.sm.Broadcast(s.jobID, sse.EventPageEnriched, map[string]any{
		"url":       url,
		"page_type": pageType,
		"title":     title,
	})
}
