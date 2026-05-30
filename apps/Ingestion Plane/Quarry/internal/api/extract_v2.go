package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"regexp"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/asyncjobs"
	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/extractor"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
	quarrysearch "github.com/triodelab/quarry/internal/search"
	"github.com/triodelab/quarry/internal/sse"
)

// ---------- v2 Extract types ----------

type V2ExtractRequest struct {
	URLs               []string                `json:"urls"`
	Preset             string                  `json:"preset,omitempty"`
	Prompt             string                  `json:"prompt,omitempty"`
	SystemPrompt       string                  `json:"systemPrompt,omitempty"`
	Schema             json.RawMessage         `json:"schema,omitempty"`
	EnableWebSearch    bool                    `json:"enableWebSearch,omitempty"`
	Limit              int                     `json:"limit,omitempty"`
	TimeoutSec         int                     `json:"timeout,omitempty"`
	IgnoreInvalidURLs  bool                    `json:"ignoreInvalidURLs,omitempty"`
	IgnoreSitemap      bool                    `json:"ignoreSitemap,omitempty"`
	Sitemap            quarrycrawl.SitemapMode `json:"sitemap,omitempty"`
	IncludePaths       []string                `json:"includePaths,omitempty"`
	ExcludePaths       []string                `json:"excludePaths,omitempty"`
	IncludeSubdomains  bool                    `json:"includeSubdomains,omitempty"`
	AllowExternalLinks bool                    `json:"allowExternalLinks,omitempty"`
	IgnoreRobotsTxt    bool                    `json:"ignoreRobotsTxt,omitempty"`
	MaxDiscoveryDepth  *int                    `json:"maxDiscoveryDepth,omitempty"`
	Webhook            *models.WebhookConfig   `json:"webhook,omitempty"`
	ScrapeOptions      *v2ScrapeOptionsRequest `json:"scrapeOptions,omitempty"`
}

type V2ExtractResponse struct {
	Success     bool                   `json:"success"`
	ID          string                 `json:"id,omitempty"`
	JobID       string                 `json:"job_id,omitempty"`
	Status      string                 `json:"status"`
	Data        map[string]interface{} `json:"data,omitempty"`
	Error       string                 `json:"error,omitempty"`
	URLTrace    []string               `json:"urlTrace,omitempty"`
	InvalidURLs []string               `json:"invalidURLs,omitempty"`
}

type v2ExtractRunConfig struct {
	Preset       string
	OrgID        string
	Schema       string
	Prompt       string
	SystemPrompt string
	TimeoutSec   int
	URLTrace     []string
	Webhook      *models.WebhookConfig
	ScrapeFormat *scraper.FormatOptions
}

// ---------- v2 Search types ----------

type V2SearchRequest struct {
	Query         string                  `json:"query"`
	Preset        string                  `json:"preset,omitempty"`
	BlendMode     string                  `json:"blendMode,omitempty"`
	Limit         int                     `json:"limit,omitempty"`
	Sources       []json.RawMessage       `json:"sources,omitempty"`
	Scrape        bool                    `json:"scrape,omitempty"`
	Formats       []string                `json:"formats,omitempty"`
	ScrapeOptions *v2ScrapeOptionsRequest `json:"scrapeOptions,omitempty"`
	AsyncScraping bool                    `json:"asyncScraping,omitempty"`
	Webhook       *models.WebhookConfig   `json:"webhook,omitempty"`
	TimeoutSec    int                     `json:"timeout,omitempty"`
}

type v2SearchSource struct {
	Type          string
	Site          string
	Weight        float64
	Limit         int
	Country       string
	SearchLang    string
	UILang        string
	Freshness     string
	SafeSearch    string
	ExtraSnippets bool
}

type V2SearchResult struct {
	Title   string  `json:"title"`
	URL     string  `json:"url"`
	Snippet string  `json:"snippet"`
	Source  string  `json:"source"`
	Type    string  `json:"type"`
	Content string  `json:"content,omitempty"`
	Score   float64 `json:"score,omitempty"`
}

// V2SearchBuckets groups search results by source type, mirroring the
// Firecrawl v2 search response shape.
type V2SearchBuckets struct {
	Web    []V2SearchResult `json:"web"`
	News   []V2SearchResult `json:"news"`
	Images []V2SearchResult `json:"images"`
}

type V2SearchResponse struct {
	Success bool            `json:"success"`
	Query   string          `json:"query"`
	Data    V2SearchBuckets `json:"data"`
	Count   int             `json:"count"`
}

type v2SearchCreateResponse struct {
	Success bool   `json:"success"`
	ID      string `json:"id"`
	JobID   string `json:"job_id"`
	Status  string `json:"status"`
	Query   string `json:"query"`
}

type v2SearchStatusResponse struct {
	Success   bool             `json:"success"`
	ID        string           `json:"id"`
	JobID     string           `json:"job_id"`
	Status    string           `json:"status"`
	Query     string           `json:"query"`
	Completed int              `json:"completed"`
	Total     int              `json:"total"`
	Count     int              `json:"count"`
	ExpiresAt time.Time        `json:"expiresAt"`
	Next      string           `json:"next,omitempty"`
	Data      []V2SearchResult `json:"data"`
	Error     string           `json:"error,omitempty"`
}

type v2SearchRunConfig struct {
	Preset       string
	OrgID        string
	BlendMode    string
	Query        string
	Limit        int
	Sources      []v2SearchSource
	ScrapeOpts   *scraper.FormatOptions
	ShouldScrape bool
	TimeoutSec   int
	Webhook      *models.WebhookConfig
}

func (h *Handler) registerV2ExtractAndSearch(v2 fiber.Router) {
	v2.Post("/extract", h.v2Extract)
	v2.Get("/extract/:id", h.v2ExtractStatus)
	v2.Post("/search", h.v2Search)
	v2.Get("/search/:id", h.v2SearchStatus)
	v2.Delete("/search/:id", h.v2CancelSearch)
}

// ---------- POST /v2/extract ----------

func (h *Handler) v2Extract(c *fiber.Ctx) error {
	var req V2ExtractRequest
	if err := json.Unmarshal(c.Body(), &req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	req.Prompt = strings.TrimSpace(req.Prompt)
	req.SystemPrompt = strings.TrimSpace(req.SystemPrompt)
	if req.Webhook != nil && strings.TrimSpace(req.Webhook.URL) != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}

	schema, err := normalizeOptionalSchema(req.Schema)
	if err != nil {
		return writeError(c, http.StatusBadRequest, "schema must be valid JSON or a JSON-encoded string", nil)
	}
	if len(req.URLs) == 0 && req.Prompt == "" {
		return writeError(c, http.StatusBadRequest, "urls or prompt is required", nil)
	}

	if req.Limit <= 0 {
		req.Limit = 5
	}
	if req.Limit > 50 {
		req.Limit = 50
	}
	if req.TimeoutSec <= 0 {
		req.TimeoutSec = 60
	}
	if req.TimeoutSec > 300 {
		req.TimeoutSec = 300
	}

	scrapeOpts, err := buildV2ExtractScrapeOptions(req.ScrapeOptions)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if scrapeOpts != nil && req.ScrapeOptions != nil {
		scrapeOpts.ProxyURL = h.resolveProxyURL(c, req.ScrapeOptions.Proxy)
	}

	resolvedURLs, invalidURLs, err := h.resolveV2ExtractURLs(c.UserContext(), req)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}

	if req.EnableWebSearch && len(resolvedURLs) == 0 && req.Prompt != "" && h.searchClient != nil {
		searchCtx, searchCancel := context.WithTimeout(c.UserContext(), 10*time.Second)
		defer searchCancel()
		results, searchErr := h.searchClient.Search(searchCtx, quarrysearch.SearchTypeWeb, quarrysearch.SearchOptions{
			Query: req.Prompt,
			Limit: req.Limit,
		})
		if searchErr == nil {
			searchURLs := make([]string, 0, len(results))
			for _, result := range results {
				searchURLs = append(searchURLs, result.URL)
			}
			resolvedURLs, _, err = normalizeExtractURLs(searchURLs, true)
			if err != nil {
				return writeError(c, http.StatusBadRequest, err.Error(), nil)
			}
		}
	}

	if len(resolvedURLs) == 0 {
		if len(invalidURLs) > 0 {
			return writeError(c, http.StatusBadRequest, "no valid URLs resolved", map[string]interface{}{"invalidURLs": invalidURLs})
		}
		return writeError(c, http.StatusBadRequest, "no URLs resolved (provide urls or enable web search with a prompt)", nil)
	}

	if len(resolvedURLs) > req.Limit {
		resolvedURLs = resolvedURLs[:req.Limit]
	}

	orgID, userID, _ := h.currentOrgAndUser(c)

	jobID := "v2extract_" + uuid.NewString()[:8]
	if h.jobStore != nil {
		h.jobStore.Upsert(&jobs.Job{
			ID:        jobID,
			Status:    jobs.StatusPending,
			CreatedAt: time.Now(),
			ExpiresAt: time.Now().Add(time.Hour),
			Meta: map[string]string{
				"kind":        "v2_extract",
				"api_version": "v2",
			},
			Result: map[string]any{
				"urlTrace":    append([]string(nil), resolvedURLs...),
				"invalidURLs": append([]string(nil), invalidURLs...),
				"completed":   0,
				"total":       len(resolvedURLs),
			},
		})
	}
	if h.extractionJobStore != nil {
		if err := h.extractionJobStore.Create(c.UserContext(), &jobs.ExtractionJob{
			ID:     jobID,
			URL:    strings.Join(resolvedURLs, ","),
			Schema: schema,
			Prompt: req.Prompt,
			Status: jobs.ExtractionQueued,
		}); err != nil {
			return writeError(c, http.StatusInternalServerError, "failed to queue extraction job", err.Error())
		}
	}
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobCreated, map[string]any{
			"jobId":       jobID,
			"resource":    "extract",
			"urlTrace":    resolvedURLs,
			"invalidURLs": invalidURLs,
		})
	}

	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindExtract, jobID, asyncjobs.ExtractPayload{
		OrgID:        orgID,
		UserID:       userID,
		Schema:       schema,
		Prompt:       req.Prompt,
		SystemPrompt: req.SystemPrompt,
		TimeoutSec:   req.TimeoutSec,
		URLTrace:     append([]string(nil), resolvedURLs...),
		Webhook:      req.Webhook,
		ScrapeFormat: scrapeOpts,
	}, "v2"); err != nil {
		markJobDispatchFailure(h.jobStore, jobID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue extraction job", err.Error())
	}

	return c.Status(http.StatusOK).JSON(V2ExtractResponse{
		Success:     true,
		ID:          jobID,
		JobID:       jobID,
		Status:      "processing",
		URLTrace:    resolvedURLs,
		InvalidURLs: invalidURLs,
	})
}

func (h *Handler) runV2Extraction(parent context.Context, jobID string, cfg v2ExtractRunConfig) {
	defer h.activeExtractCancels.Delete(jobID)

	ctx, cancel := context.WithTimeout(parent, time.Duration(cfg.TimeoutSec)*time.Second)
	defer cancel()

	h.updateExtractLifecycle(ctx, jobID, jobs.ExtractionProcessing, 0, len(cfg.URLTrace), nil, "")
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobStarted, map[string]any{"progress": 0})
	}

	combined := make(map[string]interface{})
	var lastErr error

	for index, targetURL := range cfg.URLTrace {
		if ctx.Err() != nil {
			lastErr = ctx.Err()
			break
		}

		result, err := h.extractV2URL(ctx, targetURL, cfg)
		if err != nil {
			lastErr = err
			logEvent := zlog.Warn().Err(err)
			if !h.isJobZDR(jobID) {
				logEvent = logEvent.Str("url", targetURL)
			}
			logEvent.Msg("v2 extract: extraction failed")
			continue
		}
		combined[targetURL] = result
		h.updateExtractLifecycle(ctx, jobID, jobs.ExtractionProcessing, len(combined), len(cfg.URLTrace), combined, "")
		if h.streamManager != nil {
			h.streamManager.Broadcast(jobID, sse.EventJobProgress, map[string]any{
				"completed": len(combined),
				"total":     len(cfg.URLTrace),
				"progress":  progressPercent(index+1, len(cfg.URLTrace)),
			})
		}
	}

	if errors.Is(ctx.Err(), context.Canceled) || errors.Is(lastErr, context.Canceled) {
		h.updateExtractLifecycle(context.Background(), jobID, jobs.ExtractionCancelled, len(combined), len(cfg.URLTrace), combined, "cancelled by user")
		if h.streamManager != nil {
			h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"status": "cancelled"})
		}
		return
	}

	if len(combined) == 0 {
		errMsg := "extraction produced no results"
		if lastErr != nil {
			errMsg = lastErr.Error()
		}
		h.updateExtractLifecycle(ctx, jobID, jobs.ExtractionFailed, 0, len(cfg.URLTrace), nil, errMsg)
		if h.sharedPublisher != nil {
			_ = h.sharedPublisher.PublishCrawlFailed(context.Background(), "", "", jobID, errMsg, map[string]interface{}{
				"type":        "v2_extract",
				"api_version": "v2",
			})
		}
		if cfg.Webhook != nil && cfg.Webhook.URL != "" {
			go h.fireWebhook(cfg.Webhook.URL, &models.WebhookPayload{
				Success: false,
				Type:    "extract.failed",
				ID:      jobID,
				Error:   errMsg,
			})
		}
		if h.streamManager != nil {
			h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"error": errMsg})
		}
		return
	}

	h.updateExtractLifecycle(ctx, jobID, jobs.ExtractionCompleted, len(combined), len(cfg.URLTrace), combined, "")

	if h.sharedPublisher != nil {
		_ = h.sharedPublisher.PublishCrawlCompleted(context.Background(), "", "", jobID, len(combined), map[string]interface{}{
			"type":        "v2_extract",
			"api_version": "v2",
		})
	}
	if cfg.Webhook != nil && cfg.Webhook.URL != "" {
		go h.fireWebhook(cfg.Webhook.URL, &models.WebhookPayload{
			Success: true,
			Type:    "extract.completed",
			ID:      jobID,
		})
	}
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobCompleted, map[string]any{"progress": 100})
	}

	zlog.Info().Str("job_id", jobID).Int("urls", len(cfg.URLTrace)).Msg("v2 extract: completed")
}

func (h *Handler) extractV2URL(ctx context.Context, targetURL string, cfg v2ExtractRunConfig) (interface{}, error) {
	// Inject agent-assist fields so formats.go can trigger AI-driven interaction
	// when the page renders with insufficient content.
	if cfg.ScrapeFormat == nil {
		cfg.ScrapeFormat = &scraper.FormatOptions{Formats: []string{"html", "markdown"}}
	}
	if cfg.ScrapeFormat.AgentGoal == "" && strings.TrimSpace(cfg.Prompt) != "" {
		cfg.ScrapeFormat.AgentGoal = cfg.Prompt
	}
	if cfg.ScrapeFormat.AgentSchema == "" {
		cfg.ScrapeFormat.AgentSchema = cfg.Schema
	}
	if cfg.ScrapeFormat.OrgID == "" {
		cfg.ScrapeFormat.OrgID = cfg.OrgID
	}

	outputs, _, err := h.fetchFormats(ctx, targetURL, cfg.ScrapeFormat)
	if err != nil {
		return nil, err
	}

	html, _ := outputs["html"].(string)
	if strings.TrimSpace(cfg.Schema) != "" || strings.TrimSpace(cfg.Prompt) != "" {
		if strings.TrimSpace(html) != "" {
			resp, aiErr := h.extractAIData(ctx, &ai.ExtractRequest{
				HTML:         html,
				Schema:       cfg.Schema,
				Prompt:       cfg.Prompt,
				SystemPrompt: cfg.SystemPrompt,
				URL:          targetURL,
				OrgID:        cfg.OrgID,
			})
			if aiErr == nil && resp != nil && strings.TrimSpace(resp.Data) != "" {
				decoded := decodeV2ExtractData(resp.Data)

				// Validate extracted JSON against the caller's schema when provided.
				if strings.TrimSpace(cfg.Schema) != "" {
					if rawDecoded, encErr := json.Marshal(decoded); encErr == nil {
						validationErrs := extractor.ValidateAgainstSchema(rawDecoded, json.RawMessage(cfg.Schema))
						if len(validationErrs) > 0 {
							warning := extractor.FormatValidationWarning(validationErrs)
							zlog.Warn().Str("url", targetURL).Str("warning", warning).Msg("v2 extract: schema validation warnings")
							// Return the data with a _validation_warnings key so callers can inspect.
							if dataMap, ok := decoded.(map[string]interface{}); ok {
								dataMap["_validation_warnings"] = extractor.ValidationErrorStrings(validationErrs)
								return dataMap, nil
							}
						}
					}
				}

				return decoded, nil
			}
			if aiErr != nil {
				zlog.Warn().Err(aiErr).Str("url", targetURL).Msg("v2 extract: AI extraction failed, using fallback content")
			}
		}
	}

	if markdown, _ := outputs["markdown"].(string); strings.TrimSpace(markdown) != "" {
		return map[string]interface{}{
			"content": markdown,
			"url":     targetURL,
		}, nil
	}
	if strings.TrimSpace(html) != "" {
		return map[string]interface{}{
			"content": html,
			"url":     targetURL,
		}, nil
	}

	return nil, fmt.Errorf("no extractable content returned")
}

func (h *Handler) updateV2ExtractJob(ctx context.Context, jobID string, mutate func(*jobs.ExtractionJob)) {
	if h.extractionJobStore == nil || mutate == nil {
		return
	}
	job, err := h.extractionJobStore.Get(ctx, jobID)
	if err != nil || job == nil {
		return
	}
	mutate(job)
	_ = h.extractionJobStore.Update(ctx, job)
}

func (h *Handler) updateExtractLifecycle(ctx context.Context, jobID string, status jobs.ExtractionStatus, completed, total int, result map[string]interface{}, errMsg string) {
	if h.isJobZDR(jobID) {
		result = sanitizeExtractPayloadForZDR(result, completed, total)
	}
	h.updateV2ExtractJob(ctx, jobID, func(job *jobs.ExtractionJob) {
		job.Status = status
		job.Error = errMsg
		job.Result = mergeResearchResult(job.Result, mapFromAny(result))
	})
	if h.jobStore == nil {
		return
	}
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = mapExtractJobStatus(status)
		current.Progress = progressPercent(completed, total)
		current.Error = errMsg
		existing := current.Result
		if existing == nil {
			existing = map[string]any{}
		}
		existing["completed"] = completed
		existing["total"] = total
		if result != nil {
			existing["data"] = result
		}
		current.Result = existing
	})
}

func mapExtractJobStatus(status jobs.ExtractionStatus) jobs.Status {
	switch status {
	case jobs.ExtractionCompleted:
		return jobs.StatusReady
	case jobs.ExtractionFailed:
		return jobs.StatusFailed
	case jobs.ExtractionCancelled:
		return jobs.StatusCancelled
	default:
		return jobs.StatusRunning
	}
}

// ---------- GET /v2/extract/:id ----------

func (h *Handler) v2ExtractStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.extractionJobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "extraction store not initialized", nil)
	}

	job, err := h.extractionJobStore.Get(c.UserContext(), jobID)
	if err != nil {
		return writeError(c, http.StatusNotFound, "extraction job not found", nil)
	}

	status := string(job.Status)
	if status == "" {
		status = "processing"
	}

	return c.JSON(V2ExtractResponse{
		Success: true,
		ID:      jobID,
		JobID:   jobID,
		Status:  status,
		Data:    job.Result,
		Error:   job.Error,
	})
}

// ---------- POST /v2/search ----------

func (h *Handler) v2Search(c *fiber.Ctx) error {
	var req V2SearchRequest
	if err := json.Unmarshal(c.Body(), &req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}
	if strings.TrimSpace(req.Query) == "" {
		return writeError(c, http.StatusBadRequest, "query is required", nil)
	}
	if req.Limit <= 0 {
		req.Limit = 10
	}
	if req.Limit > 50 {
		req.Limit = 50
	}
	if req.TimeoutSec <= 0 {
		req.TimeoutSec = 15
	}
	if req.TimeoutSec > 300 {
		req.TimeoutSec = 300
	}
	req.BlendMode = normalizeBlendMode(req.BlendMode)
	if req.BlendMode == "" {
		return writeError(c, http.StatusBadRequest, "blendMode contains unsupported value", nil)
	}
	if req.Webhook != nil && strings.TrimSpace(req.Webhook.URL) != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}

	sources, err := parseV2SearchSources(req.Sources)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if err := h.ensureSearchSourcesAvailable(sources); err != nil {
		return writeError(c, http.StatusServiceUnavailable, err.Error(), nil)
	}
	orgID, _, _ := h.currentOrgAndUser(c)

	scrapeOpts, shouldScrape, err := buildV2SearchScrapeOptions(req)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if scrapeOpts != nil && req.ScrapeOptions != nil {
		scrapeOpts.ProxyURL = h.resolveProxyURL(c, req.ScrapeOptions.Proxy)
	}

	asyncMode := req.AsyncScraping || req.Webhook != nil
	if asyncMode {
		if h.jobStore == nil || h.searchAsyncStore == nil {
			return writeError(c, http.StatusServiceUnavailable, "search async store is not initialized", nil)
		}

		job := h.jobStore.New(map[string]string{
			"kind":        "v2_search",
			"query":       req.Query,
			"api_version": "v2",
		})
		if h.streamManager != nil {
			h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{
				"jobId":    job.ID,
				"resource": "search",
				"query":    req.Query,
			})
		}
		if err := h.searchAsyncStore.CreateRun(c.UserContext(), &quarrysearch.AsyncRun{
			ID:        job.ID,
			Query:     req.Query,
			CreatedAt: job.CreatedAt,
			UpdatedAt: job.UpdatedAt,
			ExpiresAt: job.ExpiresAt,
			Completed: 0,
			Total:     0,
		}); err != nil {
			return writeError(c, http.StatusInternalServerError, "failed to initialize search job", err.Error())
		}
		_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusPending
			current.Result = map[string]any{
				"query":     req.Query,
				"count":     0,
				"completed": 0,
				"total":     0,
			}
		})
		if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindSearch, job.ID, asyncjobs.SearchPayload{
			OrgID:        orgID,
			BlendMode:    req.BlendMode,
			Query:        req.Query,
			Limit:        req.Limit,
			Sources:      toAsyncSearchSources(sources),
			ScrapeOpts:   scrapeOpts,
			ShouldScrape: shouldScrape,
			TimeoutSec:   req.TimeoutSec,
			Webhook:      req.Webhook,
		}, "v2"); err != nil {
			markJobDispatchFailure(h.jobStore, job.ID, err)
			return writeError(c, http.StatusBadGateway, "failed to queue search job", err.Error())
		}

		return c.JSON(v2SearchCreateResponse{
			Success: true,
			ID:      job.ID,
			JobID:   job.ID,
			Status:  "processing",
			Query:   req.Query,
		})
	}

	searchCtx, searchCancel := context.WithTimeout(c.UserContext(), time.Duration(req.TimeoutSec)*time.Second)
	defer searchCancel()
	v2Results, err := h.executeV2Search(searchCtx, v2SearchRunConfig{
		OrgID:        orgID,
		BlendMode:    req.BlendMode,
		Query:        req.Query,
		Limit:        req.Limit,
		Sources:      sources,
		ScrapeOpts:   scrapeOpts,
		ShouldScrape: shouldScrape,
		TimeoutSec:   req.TimeoutSec,
	}, nil)
	if err != nil {
		return writeError(c, http.StatusBadGateway, "search returned no results", err.Error())
	}

	return c.JSON(V2SearchResponse{
		Success: true,
		Query:   req.Query,
		Data:    bucketV2SearchResults(v2Results),
		Count:   len(v2Results),
	})
}

func (h *Handler) v2SearchStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil || h.searchAsyncStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "search async store is not initialized", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "v2_search" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	run, err := h.searchAsyncStore.GetRun(c.UserContext(), jobID)
	if err != nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}
	skip := max(c.QueryInt("skip", 0), 0)
	limit := c.QueryInt("limit", 0)
	results, totalResults, err := h.searchAsyncStore.ListResults(c.UserContext(), jobID, skip, limit)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to retrieve search results", err.Error())
	}
	data := fromStoredSearchResults(results)
	completed := intFromAny(job.Result["completed"])
	total := intFromAny(job.Result["total"])
	count := intFromAny(job.Result["count"])
	if count == 0 && completed > 0 {
		count = completed
	}
	if completed == 0 && len(data) > 0 && job.Status == jobs.StatusReady {
		completed = run.Completed
	}
	if total == 0 && run.Total > 0 {
		total = run.Total
	}
	if total == 0 {
		total = totalResults
	}
	next := buildSearchNextURL(jobID, skip, limit, totalResults, len(data))
	expiresAt := job.ExpiresAt
	if run != nil && !run.ExpiresAt.IsZero() {
		expiresAt = run.ExpiresAt
	}

	return c.JSON(v2SearchStatusResponse{
		Success:   true,
		ID:        job.ID,
		JobID:     job.ID,
		Status:    mapV2SearchJobStatus(job.Status),
		Query:     stringFromAny(job.Result["query"], job.Meta["query"]),
		Completed: completed,
		Total:     total,
		Count:     len(data),
		ExpiresAt: expiresAt,
		Next:      next,
		Data:      data,
		Error:     job.Error,
	})
}

func (h *Handler) v2CancelSearch(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "v2_search" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	if cancelValue, ok := h.activeSearchCancels.Load(jobID); ok {
		if cancelFn, ok := cancelValue.(context.CancelFunc); ok {
			cancelFn()
		}
	}
	h.publishAsyncCancel(c.UserContext(), asyncjobs.KindSearch, jobID)
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusCancelled
		current.Error = "cancelled by user"
	})

	return c.JSON(fiber.Map{
		"success": true,
		"id":      jobID,
		"job_id":  jobID,
		"status":  "cancelled",
	})
}

func (h *Handler) runV2SearchJob(ctx context.Context, jobID string, cfg v2SearchRunConfig) {
	defer h.activeSearchCancels.Delete(jobID)

	runCtx, cancel := context.WithTimeout(ctx, time.Duration(cfg.TimeoutSec)*time.Second)
	defer cancel()
	storedQuery := cfg.Query
	if job, ok := h.jobStore.Get(jobID); ok && job != nil && strings.EqualFold(strings.TrimSpace(job.Meta["zdr_mode"]), "true") {
		storedQuery = ""
	}
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobStarted, map[string]any{"progress": 0})
	}

	results, err := h.executeV2Search(runCtx, cfg, func(completed, total int, partial []V2SearchResult) {
		storedResults := toStoredSearchResults(partial)
		if h.isJobZDR(jobID) {
			storedResults = sanitizeStoredSearchResultsForZDR(storedResults)
		}
		if run, runErr := h.searchAsyncStore.GetRun(context.Background(), jobID); runErr == nil && run != nil {
			run.Completed = completed
			run.Total = total
			_ = h.searchAsyncStore.SetRun(context.Background(), run)
		}
		_ = h.searchAsyncStore.ReplaceResults(context.Background(), jobID, storedResults)
		_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
			current.Status = jobs.StatusRunning
			current.Progress = progressPercent(completed, total)
			current.Result = map[string]any{
				"preset":    cfg.Preset,
				"query":     storedQuery,
				"count":     completed,
				"completed": completed,
				"total":     total,
			}
		})
		if h.streamManager != nil {
			h.streamManager.Broadcast(jobID, sse.EventJobProgress, map[string]any{
				"completed": completed,
				"total":     total,
				"progress":  progressPercent(completed, total),
			})
		}
	})
	if err != nil {
		if errors.Is(err, context.Canceled) {
			_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
				current.Status = jobs.StatusCancelled
				if current.Error == "" {
					current.Error = "cancelled by user"
				}
			})
			if h.streamManager != nil {
				h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"status": "cancelled"})
			}
			return
		}

		_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = err.Error()
		})
		if h.streamManager != nil {
			h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"error": err.Error()})
		}
		if cfg.Webhook != nil && cfg.Webhook.URL != "" {
			go h.fireWebhook(cfg.Webhook.URL, &models.WebhookPayload{
				Success: false,
				Type:    "search.failed",
				ID:      jobID,
				Error:   err.Error(),
			})
		}
		return
	}

	finalResults := toStoredSearchResults(results)
	if h.isJobZDR(jobID) {
		finalResults = sanitizeStoredSearchResultsForZDR(finalResults)
	}
	_ = h.searchAsyncStore.ReplaceResults(context.Background(), jobID, finalResults)
	if run, runErr := h.searchAsyncStore.GetRun(context.Background(), jobID); runErr == nil && run != nil {
		run.Completed = len(results)
		run.Total = len(results)
		_ = h.searchAsyncStore.SetRun(context.Background(), run)
	}
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusReady
		current.Progress = 100
		current.Error = ""
		current.Result = map[string]any{
			"preset":    cfg.Preset,
			"query":     storedQuery,
			"count":     len(results),
			"completed": len(results),
			"total":     len(results),
		}
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobCompleted, map[string]any{"progress": 100})
	}
	if cfg.Webhook != nil && cfg.Webhook.URL != "" {
		go h.fireWebhook(cfg.Webhook.URL, &models.WebhookPayload{
			Success: true,
			Type:    "search.completed",
			ID:      jobID,
		})
	}
}

// bucketV2SearchResults partitions a flat result list into typed buckets
// by source type ("web", "news", "images"). Unknown source types fall into web.
func bucketV2SearchResults(results []V2SearchResult) V2SearchBuckets {
	b := V2SearchBuckets{
		Web:    make([]V2SearchResult, 0),
		News:   make([]V2SearchResult, 0),
		Images: make([]V2SearchResult, 0),
	}
	for _, r := range results {
		switch r.Type {
		case "news":
			b.News = append(b.News, r)
		case "image", "images":
			b.Images = append(b.Images, r)
		default:
			b.Web = append(b.Web, r)
		}
	}
	return b
}

func (h *Handler) executeV2Search(ctx context.Context, cfg v2SearchRunConfig, progress func(int, int, []V2SearchResult)) ([]V2SearchResult, error) {
	resultsBySource := make([][]quarrysearch.Result, 0, len(cfg.Sources))
	usedSources := make([]v2SearchSource, 0, len(cfg.Sources))
	for _, source := range cfg.Sources {
		results, searchErr := h.executeSearchSource(ctx, cfg.OrgID, cfg, source)
		if searchErr != nil {
			zlog.Warn().Err(searchErr).Str("source", source.Type).Msg("v2 search: source failed")
			continue
		}
		resultsBySource = append(resultsBySource, results)
		usedSources = append(usedSources, source)
	}
	allResults := blendSearchResults(resultsBySource, usedSources, cfg.BlendMode, cfg.Limit)
	if len(allResults) == 0 {
		return nil, fmt.Errorf("search returned no results")
	}

	v2Results := make([]V2SearchResult, 0, len(allResults))
	total := len(allResults)
	for index, result := range allResults {
		if err := ctx.Err(); err != nil {
			return nil, err
		}

		item := V2SearchResult{
			Title:   result.Title,
			URL:     result.URL,
			Snippet: result.Snippet,
			Source:  result.Source,
			Type:    result.Type,
			Score:   result.Score,
		}
		if cfg.ShouldScrape && isScrapableSearchURL(result.URL) {
			outputs, _, fetchErr := h.fetchFormats(ctx, result.URL, cfg.ScrapeOpts)
			if fetchErr == nil {
				item.Content = firstSearchContent(outputs)
			} else if errors.Is(fetchErr, context.Canceled) {
				return nil, fetchErr
			}
		}
		v2Results = append(v2Results, item)
		if progress != nil {
			progress(index+1, total, append([]V2SearchResult(nil), v2Results...))
		}
	}

	return v2Results, nil
}

// ---------- Helpers ----------

var wildcardSuffix = regexp.MustCompile(`/\*$|\*$`)

func (h *Handler) resolveV2ExtractURLs(ctx context.Context, req V2ExtractRequest) ([]string, []string, error) {
	seen := make(map[string]struct{}, len(req.URLs))
	resolved := make([]string, 0, len(req.URLs))
	invalid := make([]string, 0)

	for _, rawURL := range req.URLs {
		trimmed := strings.TrimSpace(rawURL)
		if trimmed == "" {
			continue
		}

		if wildcardSuffix.MatchString(trimmed) {
			discovered, discoverErr := h.discoverV2ExtractWildcardURLs(ctx, trimmed, req)
			if discoverErr != nil {
				if !req.IgnoreInvalidURLs {
					return nil, nil, discoverErr
				}
				invalid = append(invalid, trimmed)
				continue
			}
			for _, item := range discovered {
				if _, exists := seen[item]; exists {
					continue
				}
				seen[item] = struct{}{}
				resolved = append(resolved, item)
			}
			continue
		}

		valid, invalidURLs, normalizeErr := normalizeExtractURLs([]string{trimmed}, req.IgnoreInvalidURLs)
		if normalizeErr != nil {
			return nil, nil, normalizeErr
		}
		invalid = append(invalid, invalidURLs...)
		for _, item := range valid {
			if _, exists := seen[item]; exists {
				continue
			}
			seen[item] = struct{}{}
			resolved = append(resolved, item)
		}
	}

	return resolved, invalid, nil
}

func (h *Handler) discoverV2ExtractWildcardURLs(ctx context.Context, raw string, req V2ExtractRequest) ([]string, error) {
	baseURL := wildcardSuffix.ReplaceAllString(strings.TrimSpace(raw), "")
	if err := validateAbsoluteHTTPURL(baseURL); err != nil {
		return nil, fmt.Errorf("invalid url: %v", err)
	}

	sitemapMode, err := normalizeV2ExtractSitemapMode(req)
	if err != nil {
		return nil, err
	}

	spec, err := quarrycrawl.NormalizeSpec(quarrycrawl.NormalizeInput{
		URL:                baseURL,
		Limit:              req.Limit,
		MaxDiscoveryDepth:  req.MaxDiscoveryDepth,
		AllowSubdomains:    req.IncludeSubdomains,
		AllowExternalLinks: req.AllowExternalLinks,
		IgnoreRobotsTxt:    req.IgnoreRobotsTxt,
		Sitemap:            sitemapMode,
		DiscoveryOnly:      true,
	})
	if err != nil {
		return nil, fmt.Errorf("invalid url: %v", err)
	}

	filterSpec := spec
	filterSpec.IncludePaths = append([]string(nil), req.IncludePaths...)
	filterSpec.ExcludePaths = append([]string(nil), req.ExcludePaths...)
	matcher, err := quarrycrawl.NewMatcher(filterSpec)
	if err != nil {
		return nil, err
	}

	discovered, err := h.sampleCrawlURLs(ctx, spec)
	if err != nil {
		return nil, err
	}

	filtered := make([]string, 0, len(discovered))
	for _, item := range discovered {
		if item == baseURL {
			continue
		}
		if !matcher.Match(item) {
			continue
		}
		filtered = append(filtered, item)
	}
	return filtered, nil
}

func normalizeV2ExtractSitemapMode(req V2ExtractRequest) (quarrycrawl.SitemapMode, error) {
	mode := quarrycrawl.SitemapMode(strings.ToLower(strings.TrimSpace(string(req.Sitemap))))
	if mode == "" {
		if req.IgnoreSitemap {
			return quarrycrawl.SitemapSkip, nil
		}
		return quarrycrawl.SitemapInclude, nil
	}

	switch mode {
	case quarrycrawl.SitemapInclude, quarrycrawl.SitemapSkip, quarrycrawl.SitemapOnly:
		return mode, nil
	default:
		return "", fmt.Errorf("sitemap must be one of include, skip, or only")
	}
}

func normalizeExtractURLs(urls []string, ignoreInvalid bool) ([]string, []string, error) {
	seen := make(map[string]struct{}, len(urls))
	valid := make([]string, 0, len(urls))
	invalid := make([]string, 0)

	for _, rawURL := range urls {
		trimmed := strings.TrimSpace(rawURL)
		if trimmed == "" {
			continue
		}
		if err := validateAbsoluteHTTPURL(trimmed); err != nil {
			if !ignoreInvalid {
				return nil, nil, fmt.Errorf("invalid url: %v", err)
			}
			invalid = append(invalid, trimmed)
			continue
		}
		if _, exists := seen[trimmed]; exists {
			continue
		}
		seen[trimmed] = struct{}{}
		valid = append(valid, trimmed)
	}

	return valid, invalid, nil
}

func buildV2ExtractScrapeOptions(raw *v2ScrapeOptionsRequest) (*scraper.FormatOptions, error) {
	if raw == nil {
		return &scraper.FormatOptions{Formats: []string{"html", "markdown"}}, nil
	}

	pageOptions, err := parseV2ScrapeOptions(raw)
	if err != nil {
		return nil, err
	}
	formats, _ := selectCrawlFormats(pageOptions.Formats)
	if !containsFormatName(formats, "html") {
		formats = append(formats, "html")
	}
	if !containsFormatName(formats, "markdown") {
		formats = append(formats, "markdown")
	}

	return &scraper.FormatOptions{
		Formats:         formats,
		Headers:         cloneStringMap(pageOptions.Headers),
		WaitFor:         pageOptions.WaitFor,
		MaxAgeMs:        pageOptions.MaxAgeMs,
		OnlyMainContent: pageOptions.OnlyMainContent,
		IncludeTags:     append([]string(nil), pageOptions.IncludeTags...),
		ExcludeTags:     append([]string(nil), pageOptions.ExcludeTags...),
		Actions:         pageOptions.Actions,
		Mobile:          pageOptions.Mobile,
		Viewport:        pageOptions.Viewport,
		Location:        pageOptions.Location,
		ProxyURL:        pageOptions.ProxyURL,
		BlockAds:        pageOptions.BlockAds,
		ParserMode:      pageOptions.ParserMode,
	}, nil
}

func decodeV2ExtractData(raw string) interface{} {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return raw
	}

	var decoded interface{}
	if err := json.Unmarshal([]byte(trimmed), &decoded); err == nil {
		return decoded
	}
	return raw
}

func parseV2SearchSources(raw []json.RawMessage) ([]v2SearchSource, error) {
	if len(raw) == 0 {
		return []v2SearchSource{{Type: string(quarrysearch.SearchTypeWeb)}}, nil
	}

	sources := make([]v2SearchSource, 0, len(raw))
	for _, item := range raw {
		trimmed := strings.TrimSpace(string(item))
		if trimmed == "" || trimmed == "null" {
			continue
		}

		var asString string
		if err := json.Unmarshal(item, &asString); err == nil {
			source, err := normalizeV2SearchSource(v2SearchSource{Type: asString})
			if err != nil {
				return nil, err
			}
			sources = append(sources, source)
			continue
		}

		var asObject struct {
			Type              string  `json:"type"`
			Site              string  `json:"site,omitempty"`
			Weight            float64 `json:"weight,omitempty"`
			Limit             int     `json:"limit,omitempty"`
			Country           string  `json:"country,omitempty"`
			Lang              string  `json:"lang,omitempty"`
			SearchLang        string  `json:"searchLang,omitempty"`
			SearchLangAlias   string  `json:"search_lang,omitempty"`
			UILang            string  `json:"uiLang,omitempty"`
			UILangAlias       string  `json:"ui_lang,omitempty"`
			TBS               string  `json:"tbs,omitempty"`
			Freshness         string  `json:"freshness,omitempty"`
			SafeSearch        string  `json:"safeSearch,omitempty"`
			SafeSearchAlias   string  `json:"safesearch,omitempty"`
			ExtraSnippets     bool    `json:"extraSnippets,omitempty"`
			ExtraSnippetsAlt  bool    `json:"extra_snippets,omitempty"`
			IncludeSubdomains bool    `json:"includeSubdomains,omitempty"`
		}
		if err := json.Unmarshal(item, &asObject); err != nil {
			return nil, fmt.Errorf("sources contains invalid value")
		}

		searchLang := strings.TrimSpace(asObject.SearchLang)
		if searchLang == "" {
			searchLang = strings.TrimSpace(asObject.SearchLangAlias)
		}
		if searchLang == "" {
			searchLang = strings.TrimSpace(asObject.Lang)
		}

		uiLang := strings.TrimSpace(asObject.UILang)
		if uiLang == "" {
			uiLang = strings.TrimSpace(asObject.UILangAlias)
		}

		safeSearch := strings.TrimSpace(asObject.SafeSearch)
		if safeSearch == "" {
			safeSearch = strings.TrimSpace(asObject.SafeSearchAlias)
		}

		freshness := strings.TrimSpace(asObject.Freshness)
		if freshness == "" {
			freshness = strings.TrimSpace(asObject.TBS)
		}

		source, err := normalizeV2SearchSource(v2SearchSource{
			Type:          asObject.Type,
			Site:          asObject.Site,
			Weight:        asObject.Weight,
			Limit:         asObject.Limit,
			Country:       asObject.Country,
			SearchLang:    searchLang,
			UILang:        uiLang,
			Freshness:     freshness,
			SafeSearch:    safeSearch,
			ExtraSnippets: asObject.ExtraSnippets || asObject.ExtraSnippetsAlt,
		})
		if err != nil {
			return nil, err
		}
		sources = append(sources, source)
	}

	if len(sources) == 0 {
		return []v2SearchSource{{Type: string(quarrysearch.SearchTypeWeb)}}, nil
	}
	return sources, nil
}

func normalizeV2SearchSource(source v2SearchSource) (v2SearchSource, error) {
	source.Type = strings.ToLower(strings.TrimSpace(source.Type))
	source.Site = strings.TrimSpace(source.Site)
	if source.Type == "" {
		source.Type = string(quarrysearch.SearchTypeWeb)
	}
	if source.Weight <= 0 {
		source.Weight = 1
	}
	if source.Limit < 0 {
		return v2SearchSource{}, fmt.Errorf("sources contains unsupported value")
	}
	switch source.Type {
	case string(quarrysearch.SearchTypeWeb), string(quarrysearch.SearchTypeNews), string(quarrysearch.SearchTypeImages), searchSourceDocuments, searchSourceGitHub, searchSourceIndex:
		return source, nil
	default:
		return v2SearchSource{}, fmt.Errorf("sources contains unsupported value")
	}
}

func buildV2SearchScrapeOptions(req V2SearchRequest) (*scraper.FormatOptions, bool, error) {
	if req.ScrapeOptions != nil {
		pageOptions, err := parseV2ScrapeOptions(req.ScrapeOptions)
		if err != nil {
			return nil, false, err
		}
		formats, _ := selectCrawlFormats(pageOptions.Formats)
		if len(formats) == 0 {
			formats = []string{"markdown"}
		}
		return &scraper.FormatOptions{
			Formats:         formats,
			Headers:         cloneStringMap(pageOptions.Headers),
			WaitFor:         pageOptions.WaitFor,
			MaxAgeMs:        pageOptions.MaxAgeMs,
			OnlyMainContent: pageOptions.OnlyMainContent,
			IncludeTags:     append([]string(nil), pageOptions.IncludeTags...),
			ExcludeTags:     append([]string(nil), pageOptions.ExcludeTags...),
			Actions:         pageOptions.Actions,
			Mobile:          pageOptions.Mobile,
			Viewport:        pageOptions.Viewport,
			Location:        pageOptions.Location,
			ProxyURL:        pageOptions.ProxyURL,
			BlockAds:        pageOptions.BlockAds,
			ParserMode:      pageOptions.ParserMode,
		}, true, nil
	}

	if !req.Scrape && len(req.Formats) == 0 {
		return nil, false, nil
	}

	formats := append([]string(nil), req.Formats...)
	if len(formats) == 0 {
		formats = []string{"markdown"}
	}
	return &scraper.FormatOptions{Formats: formats}, true, nil
}

func firstSearchContent(outputs map[string]interface{}) string {
	if len(outputs) == 0 {
		return ""
	}
	for _, key := range []string{"markdown", "html"} {
		if value, ok := outputs[key].(string); ok && strings.TrimSpace(value) != "" {
			return value
		}
	}
	if value, ok := outputs["json"]; ok {
		if encoded, err := json.Marshal(value); err == nil {
			return string(encoded)
		}
	}
	return ""
}

func decodeV2SearchResults(raw interface{}) []V2SearchResult {
	if raw == nil {
		return nil
	}
	encoded, err := json.Marshal(raw)
	if err != nil {
		return nil
	}
	var results []V2SearchResult
	if err := json.Unmarshal(encoded, &results); err != nil {
		return nil
	}
	return results
}

func toStoredSearchResults(results []V2SearchResult) []quarrysearch.StoredResult {
	if len(results) == 0 {
		return []quarrysearch.StoredResult{}
	}
	out := make([]quarrysearch.StoredResult, 0, len(results))
	for _, item := range results {
		out = append(out, quarrysearch.StoredResult{
			Title:   item.Title,
			URL:     item.URL,
			Snippet: item.Snippet,
			Source:  item.Source,
			Type:    item.Type,
			Content: item.Content,
			Score:   item.Score,
		})
	}
	return out
}

func fromStoredSearchResults(results []quarrysearch.StoredResult) []V2SearchResult {
	if len(results) == 0 {
		return []V2SearchResult{}
	}
	out := make([]V2SearchResult, 0, len(results))
	for _, item := range results {
		out = append(out, V2SearchResult{
			Title:   item.Title,
			URL:     item.URL,
			Snippet: item.Snippet,
			Source:  item.Source,
			Type:    item.Type,
			Content: item.Content,
			Score:   item.Score,
		})
	}
	return out
}

func buildSearchNextURL(jobID string, skip, limit, total, returned int) string {
	if limit <= 0 || returned <= 0 {
		return ""
	}
	nextSkip := skip + returned
	if nextSkip >= total {
		return ""
	}
	return fmt.Sprintf("/v2/search/%s?skip=%d&limit=%d", jobID, nextSkip, limit)
}

func intFromAny(value interface{}) int {
	switch typed := value.(type) {
	case nil:
		return 0
	case int:
		return typed
	case int32:
		return int(typed)
	case int64:
		return int(typed)
	case float32:
		return int(typed)
	case float64:
		return int(typed)
	default:
		return 0
	}
}

func stringFromAny(primary interface{}, fallback string) string {
	if value, ok := primary.(string); ok && strings.TrimSpace(value) != "" {
		return value
	}
	return fallback
}

func progressPercent(completed, total int) int {
	if total <= 0 || completed <= 0 {
		return 0
	}
	if completed >= total {
		return 100
	}
	return int(float64(completed) / float64(total) * 100)
}

func mapV2SearchJobStatus(status jobs.Status) string {
	switch status {
	case jobs.StatusReady:
		return "completed"
	case jobs.StatusFailed:
		return "failed"
	case jobs.StatusCancelled:
		return "cancelled"
	default:
		return "processing"
	}
}
