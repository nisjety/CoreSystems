package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	zlog "github.com/rs/zerolog/log"
	"golang.org/x/sync/errgroup"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/asyncjobs"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/security"
	"github.com/triodelab/quarry/internal/sse"
)

// ---------- sync handler (kept for backward compat at POST /v1/agent) ----------

func (h *Handler) agentMode(c *fiber.Ctx) error {
	var req models.AgentModeAPIRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}

	req.Objective = strings.TrimSpace(req.Objective)
	if req.Objective == "" {
		return writeError(c, http.StatusBadRequest, "objective is required", nil)
	}

	if req.URL != "" {
		if err := validateAbsoluteHTTPURL(req.URL); err != nil {
			return writeError(c, http.StatusBadRequest, err.Error(), nil)
		}
	}

	if req.UserID == "" {
		req.UserID = "quarry-agent"
	}
	if req.Tier == "" {
		req.Tier = "Basic"
	}
	if req.MaxSteps <= 0 {
		req.MaxSteps = 5
	}

	var securityAssessment *security.Assessment
	if req.URL != "" && h.security != nil {
		assessment, err := h.security.AssessURL(c.UserContext(), req.URL)
		if err != nil {
			return writeError(c, http.StatusBadRequest, "security assessment failed", err.Error())
		}
		if assessment.Blocked {
			return c.Status(http.StatusForbidden).JSON(models.AgentModeAPIResponse{
				Success:  false,
				Error:    assessment.BlockReason,
				Security: assessment,
			})
		}
		securityAssessment = assessment
	}

	var changeResult *models.ChangeTrackingResult
	changeContext := map[string]interface{}{}
	if h.changeTracker != nil && req.ChangeTrack != nil && req.ChangeTrack.Enabled && req.URL != "" && h.scraper != nil {
		formats, _, err := h.scraper.FetchFormats(c.UserContext(), req.URL, &scraper.FormatOptions{Formats: []string{"markdown"}})
		if err == nil {
			snapshot := ""
			if markdown, ok := formats["markdown"].(string); ok {
				snapshot = markdown
			} else if payload, marshalErr := json.Marshal(formats); marshalErr == nil {
				snapshot = string(payload)
			}

			if snapshot != "" {
				tracked, trackErr := h.changeTracker.Track(c.UserContext(), req.URL, snapshot, req.ChangeTrack)
				if trackErr == nil {
					changeResult = tracked
					changeContext["changeStatus"] = tracked.ChangeStatus
					if tracked.Diff != nil {
						diffText := tracked.Diff.Text
						if len(diffText) > 4000 {
							diffText = diffText[:4000]
						}
						changeContext["diff"] = diffText
					}
				}
			}
		}
	}

	if h.agentClient == nil || !h.agentClient.Enabled() {
		return writeError(c, http.StatusServiceUnavailable, "agent mode is not configured (AI_CORE_HTTP_BASE_URL)", nil)
	}

	contextPayload := map[string]interface{}{}
	for key, value := range req.Context {
		contextPayload[key] = value
	}
	if req.Module != "" {
		contextPayload["module"] = req.Module
	}
	if req.Collection != "" {
		contextPayload["collection"] = req.Collection
	}

	agentResp, err := h.agentClient.Run(c.UserContext(), &ai.AgentModeRequest{
		UserID:        req.UserID,
		Tier:          req.Tier,
		Objective:     req.Objective,
		TargetURL:     req.URL,
		Context:       contextPayload,
		MaxSteps:      req.MaxSteps,
		ChangeContext: changeContext,
	})
	if err != nil {
		return writeError(c, http.StatusBadGateway, "agent mode request failed", err.Error())
	}

	return c.JSON(models.AgentModeAPIResponse{
		Success: true,
		Data: &models.AgentModeResult{
			Content:      agentResp.Content,
			ModelUsed:    agentResp.ModelUsed,
			Intent:       agentResp.Intent,
			Confidence:   agentResp.Confidence,
			RequestID:    agentResp.RequestID,
			TotalCostUSD: agentResp.TotalCostUSD,
			DurationMs:   agentResp.DurationMs,
		},
		ChangeTracking: changeResult,
		Security:       securityAssessment,
	})
}

// ---------- async agent: create ----------

func (h *Handler) v1Agent(c *fiber.Ctx) error {
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "agent is not initialized", nil)
	}
	if h.agentClient == nil || !h.agentClient.Enabled() {
		return writeError(c, http.StatusServiceUnavailable, "agent mode is not configured (AI_CORE_HTTP_BASE_URL)", nil)
	}

	var req models.AgentModeAPIRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}

	req.Objective = strings.TrimSpace(req.Objective)
	if req.Objective == "" {
		return writeError(c, http.StatusBadRequest, "objective is required", nil)
	}

	urls := req.ResolvedURLs()
	for _, u := range urls {
		if err := validateAbsoluteHTTPURL(u); err != nil {
			return writeError(c, http.StatusBadRequest, fmt.Sprintf("invalid url %q: %s", u, err.Error()), nil)
		}
	}

	if req.Webhook != nil && strings.TrimSpace(req.Webhook.URL) != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}

	// Validate and serialize schema if provided.
	schemaStr := ""
	if req.Schema != nil {
		raw, err := json.Marshal(req.Schema)
		if err != nil {
			return writeError(c, http.StatusBadRequest, "invalid schema", err.Error())
		}
		schemaStr = string(raw)
	}

	if req.UserID == "" {
		req.UserID = "quarry-agent"
	}
	if req.Tier == "" {
		req.Tier = "Basic"
	}
	if req.MaxSteps <= 0 {
		req.MaxSteps = 5
	}

	timeoutSec := req.TimeoutSec
	if timeoutSec <= 0 {
		timeoutSec = h.agentTimeoutSec()
	}

	orgID, userID, tier := h.currentOrgAndUser(c)
	if userID != "" {
		req.UserID = userID
	}
	if tier != "" {
		req.Tier = tier
	}

	job := h.jobStore.New(map[string]string{
		"kind":        "agent",
		"api_version": "v1",
		"objective":   h.redactStoredString(c, req.Objective),
		"org_id":      orgID,
		"user_id":     req.UserID,
		"tier":        req.Tier,
	})
	_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
		current.Result = map[string]any{
			"objective":   h.redactStoredString(c, req.Objective),
			"urlsScraped": 0,
		}
	})

	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{
			"jobId":     job.ID,
			"resource":  "agent",
			"objective": req.Objective,
		})
	}

	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindAgent, job.ID, asyncjobs.AgentPayload{
		OrgID:                 orgID,
		UserID:                req.UserID,
		Tier:                  req.Tier,
		Objective:             req.Objective,
		URLs:                  urls,
		Schema:                schemaStr,
		Model:                 strings.TrimSpace(req.Model),
		MaxSteps:              req.MaxSteps,
		MaxCredits:            req.MaxCredits,
		StrictConstrainToURLs: req.StrictConstrainToURLs,
		EnableWebSearch:       req.ResolvedEnableWebSearch(),
		AllowExternalLinks:    req.ResolvedAllowExternalLinks(),
		Module:                req.Module,
		Collection:            req.Collection,
		Context:               req.Context,
		ChangeTrack:           req.ChangeTrack,
		Webhook:               req.Webhook,
		TimeoutSec:            timeoutSec,
	}, "v1"); err != nil {
		markJobDispatchFailure(h.jobStore, job.ID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue agent job", err.Error())
	}

	h.recordUserActivity(c, "agent.created", "agent", map[string]interface{}{
		"jobId":     job.ID,
		"summary":   "agent queued",
		"objective": h.redactStoredString(c, req.Objective),
	})

	return c.JSON(h.newAsyncCreateEnvelope(c, "agent", job.ID, job.CreatedAt, job.ExpiresAt, "queued", nil))
}

// ---------- async agent: status ----------

func (h *Handler) v1AgentStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "agent" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	result := decodeAgentResult(job.Result)

	return c.JSON(models.AgentStatusEnvelope{
		Success:     job.Status != jobs.StatusFailed,
		ID:          job.ID,
		Resource:    "agent",
		Status:      string(job.Status),
		CreatedAt:   job.CreatedAt.Format(time.RFC3339),
		ExpiresAt:   job.ExpiresAt.Format(time.RFC3339),
		Objective:   stringFromAny(job.Result["objective"], job.Meta["objective"]),
		URLsScraped: intFromAny(job.Result["urlsScraped"]),
		Data:        result,
		Error:       job.Error,
	})
}

// ---------- async agent: cancel ----------

func (h *Handler) v1CancelAgent(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "agent" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	if cancelValue, ok := h.activeAgentCancels.Load(jobID); ok {
		if cancelFn, ok := cancelValue.(context.CancelFunc); ok {
			cancelFn()
		}
	}
	h.publishAsyncCancel(c.UserContext(), asyncjobs.KindAgent, jobID)

	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusCancelled
		current.Error = "cancelled by user"
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"status": "cancelled"})
	}

	h.recordUserActivity(c, "agent.cancelled", "agent", map[string]interface{}{
		"jobId":   jobID,
		"summary": "agent cancelled",
	})
	return c.JSON(fiber.Map{"success": true, "id": jobID, "status": "cancelled"})
}

// ---------- async agent: runner ----------

func (h *Handler) runAgentJob(ctx context.Context, jobID string, payload asyncjobs.AgentPayload) {
	defer h.activeAgentCancels.Delete(jobID)

	timeout := time.Duration(payload.TimeoutSec) * time.Second
	if timeout <= 0 {
		timeout = 120 * time.Second
	}
	runCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobStarted, map[string]any{"progress": 0})
	}

	// 1. Scrape all target URLs concurrently.
	scrapedPages := h.scrapeAgentURLs(runCtx, jobID, payload.URLs)

	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Result["urlsScraped"] = len(scrapedPages)
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, "agent.scrape_done", map[string]any{
			"urlsScraped": len(scrapedPages),
		})
	}

	// 2. Build context for AI Core.
	contextPayload := map[string]interface{}{}
	for key, value := range payload.Context {
		contextPayload[key] = value
	}
	if payload.Module != "" {
		contextPayload["module"] = payload.Module
	}
	if payload.Collection != "" {
		contextPayload["collection"] = payload.Collection
	}

	// 3. Change tracking (first URL only, if enabled).
	var changeContext map[string]interface{}
	if h.changeTracker != nil && payload.ChangeTrack != nil && payload.ChangeTrack.Enabled && len(payload.URLs) > 0 && h.scraper != nil {
		changeContext = h.agentChangeTrack(runCtx, payload.URLs[0], payload.ChangeTrack)
	}

	// 4. Call AI Core.
	if h.agentClient == nil || !h.agentClient.Enabled() {
		h.failAgentJob(jobID, "agent mode client is not configured", payload.Webhook)
		return
	}

	agentResp, err := h.agentClient.Run(runCtx, &ai.AgentModeRequest{
		UserID:             payload.UserID,
		Tier:               payload.Tier,
		Objective:          payload.Objective,
		TargetURL:          firstString(payload.URLs),
		TargetURLs:         payload.URLs,
		Schema:             payload.Schema,
		Model:              payload.Model,
		Context:            contextPayload,
		MaxSteps:           payload.MaxSteps,
		EnableWebSearch:    payload.EnableWebSearch,
		AllowExternalLinks: payload.AllowExternalLinks,
		ChangeContext:      changeContext,
		ScrapedPages:       scrapedPages,
	})
	if err != nil {
		h.failAgentJob(jobID, fmt.Sprintf("agent execution failed: %v", err), payload.Webhook)
		return
	}

	// 5. Store result.
	result := &models.AgentModeResult{
		Content:      agentResp.Content,
		ModelUsed:    agentResp.ModelUsed,
		Intent:       agentResp.Intent,
		Confidence:   agentResp.Confidence,
		RequestID:    agentResp.RequestID,
		TotalCostUSD: agentResp.TotalCostUSD,
		DurationMs:   agentResp.DurationMs,
	}
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusReady
		current.Result["data"] = result
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobCompleted, map[string]any{
			"status": "completed",
			"data":   result,
		})
	}

	// 6. Fire webhook.
	if payload.Webhook != nil && strings.TrimSpace(payload.Webhook.URL) != "" &&
		shouldSendPlatformWebhookEvent(payload.Webhook.Events, "agent.completed") {
		go h.fireWebhook(payload.Webhook.URL, &models.WebhookPayload{
			Success:  true,
			Type:     "agent.completed",
			ID:       jobID,
			Metadata: map[string]interface{}{"data": result},
		})
	}

	zlog.Info().Str("job_id", jobID).Str("model", agentResp.ModelUsed).Msg("agent job completed")
}

// ---------- helpers ----------

func (h *Handler) scrapeAgentURLs(ctx context.Context, jobID string, urls []string) []ai.ScrapedPage {
	if len(urls) == 0 || h.scraper == nil {
		return nil
	}

	concurrency := 5
	if h.cfg != nil && h.cfg.AgentScrapeConcurrency > 0 {
		concurrency = h.cfg.AgentScrapeConcurrency
	}

	type indexedPage struct {
		idx  int
		page ai.ScrapedPage
	}

	g, gCtx := errgroup.WithContext(ctx)
	g.SetLimit(concurrency)

	results := make(chan indexedPage, len(urls))
	for i, rawURL := range urls {
		g.Go(func() error {
			formats, _, err := h.scraper.FetchFormats(gCtx, rawURL, &scraper.FormatOptions{Formats: []string{"markdown"}})
			if err != nil {
				zlog.Warn().Err(err).Str("url", rawURL).Str("job_id", jobID).Msg("agent scrape failed for url")
				return nil // non-fatal
			}
			content := ""
			if md, ok := formats["markdown"].(string); ok {
				content = md
			}
			if content == "" {
				if payload, marshalErr := json.Marshal(formats); marshalErr == nil {
					content = string(payload)
				}
			}
			results <- indexedPage{idx: i, page: ai.ScrapedPage{URL: rawURL, Content: content}}
			return nil
		})
	}
	_ = g.Wait()
	close(results)

	pages := make([]ai.ScrapedPage, 0, len(urls))
	collected := make([]indexedPage, 0, len(urls))
	for p := range results {
		collected = append(collected, p)
	}
	// Sort by original index to preserve URL order.
	for i := range collected {
		for j := i + 1; j < len(collected); j++ {
			if collected[j].idx < collected[i].idx {
				collected[i], collected[j] = collected[j], collected[i]
			}
		}
	}
	for _, c := range collected {
		pages = append(pages, c.page)
	}
	return pages
}

func (h *Handler) agentChangeTrack(ctx context.Context, targetURL string, track *models.ChangeTrackingRequest) map[string]interface{} {
	if h.scraper == nil || targetURL == "" {
		return nil
	}
	formats, _, err := h.scraper.FetchFormats(ctx, targetURL, &scraper.FormatOptions{Formats: []string{"markdown"}})
	if err != nil {
		return nil
	}
	snapshot := ""
	if md, ok := formats["markdown"].(string); ok {
		snapshot = md
	} else if payload, marshalErr := json.Marshal(formats); marshalErr == nil {
		snapshot = string(payload)
	}
	if snapshot == "" {
		return nil
	}
	tracked, trackErr := h.changeTracker.Track(ctx, targetURL, snapshot, track)
	if trackErr != nil {
		return nil
	}
	out := map[string]interface{}{
		"changeStatus": tracked.ChangeStatus,
	}
	if tracked.Diff != nil {
		diffText := tracked.Diff.Text
		if len(diffText) > 4000 {
			diffText = diffText[:4000]
		}
		out["diff"] = diffText
	}
	return out
}

func (h *Handler) failAgentJob(jobID string, errMsg string, webhook *models.WebhookConfig) {
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusFailed
		current.Error = errMsg
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"error": errMsg})
	}
	if webhook != nil && strings.TrimSpace(webhook.URL) != "" &&
		shouldSendPlatformWebhookEvent(webhook.Events, "agent.failed") {
		go h.fireWebhook(webhook.URL, &models.WebhookPayload{
			Success: false,
			Type:    "agent.failed",
			ID:      jobID,
			Error:   errMsg,
		})
	}
	zlog.Error().Str("job_id", jobID).Str("error", errMsg).Msg("agent job failed")
}

func (h *Handler) agentTimeoutSec() int {
	if h.cfg != nil && h.cfg.AgentTimeoutSec > 0 {
		return h.cfg.AgentTimeoutSec
	}
	return 120
}

func decodeAgentResult(result map[string]any) *models.AgentModeResult {
	if result == nil {
		return nil
	}
	raw, ok := result["data"]
	if !ok || raw == nil {
		return nil
	}
	data, err := json.Marshal(raw)
	if err != nil {
		return nil
	}
	var out models.AgentModeResult
	if err := json.Unmarshal(data, &out); err != nil {
		return nil
	}
	return &out
}

func firstString(ss []string) string {
	if len(ss) > 0 {
		return ss[0]
	}
	return ""
}
