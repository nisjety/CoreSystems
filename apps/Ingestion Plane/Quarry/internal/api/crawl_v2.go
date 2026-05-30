package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"sort"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/ai"
	"github.com/triodelab/quarry/internal/asyncjobs"
	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/driver"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/sse"

	"github.com/triodelab/quarry/internal/dataplane"
)

const (
	v2CrawlMaxLimit = 100000
	v2CrawlMaxDepth = 50
)

type v2CrawlRequest struct {
	URL                    string                        `json:"url"`
	Preset                 string                        `json:"preset,omitempty"`
	Module                 string                        `json:"module,omitempty"`
	Mode                   string                        `json:"mode,omitempty"`
	MaxDiscoveryDepth      *int                          `json:"maxDiscoveryDepth,omitempty"`
	MaxDepth               *int                          `json:"maxDepth,omitempty"`
	Limit                  int                           `json:"limit,omitempty"`
	CrawlEntireDomain      bool                          `json:"crawlEntireDomain,omitempty"`
	AllowExternalLinks     bool                          `json:"allowExternalLinks,omitempty"`
	AllowSubdomains        bool                          `json:"allowSubdomains,omitempty"`
	IgnoreRobotsTxt        bool                          `json:"ignoreRobotsTxt,omitempty"`
	Sitemap                quarrycrawl.SitemapMode       `json:"sitemap,omitempty"`
	DeduplicateSimilarURLs *bool                         `json:"deduplicateSimilarURLs,omitempty"`
	IgnoreQueryParameters  bool                          `json:"ignoreQueryParameters,omitempty"`
	RegexOnFullURL         bool                          `json:"regexOnFullURL,omitempty"`
	RegexPaths             bool                          `json:"regexPaths,omitempty"`
	Delay                  int                           `json:"delay,omitempty"`
	MaxConcurrency         int                           `json:"maxConcurrency,omitempty"`
	ScheduleAt             *time.Time                    `json:"scheduleAt,omitempty"`
	Prompt                 string                        `json:"prompt,omitempty"`
	Schema                 json.RawMessage               `json:"schema,omitempty"`
	ChangeTracking         *models.ChangeTrackingRequest `json:"changeTracking,omitempty"`
	Headers                map[string]string             `json:"headers,omitempty"`
	Enrich                 bool                          `json:"enrich,omitempty"`
	EnrichLimit            int                           `json:"enrichLimit,omitempty"`
	MaxAge                 int64                         `json:"maxAge,omitempty"`
	IncludePaths           []string                      `json:"includePaths,omitempty"`
	ExcludePaths           []string                      `json:"excludePaths,omitempty"`
	Webhook                *models.WebhookConfig         `json:"webhook,omitempty"`
	ScrapeOptions          *v2ScrapeOptionsRequest       `json:"scrapeOptions,omitempty"`
}

type v2ScrapeOptionsRequest struct {
	Formats         []json.RawMessage      `json:"formats,omitempty"`
	Headers         map[string]string      `json:"headers,omitempty"`
	WaitFor         int                    `json:"waitFor,omitempty"`
	OnlyMainContent bool                   `json:"onlyMainContent,omitempty"`
	IncludeTags     []string               `json:"includeTags,omitempty"`
	ExcludeTags     []string               `json:"excludeTags,omitempty"`
	Actions         []models.ActionRequest `json:"actions,omitempty"`
	Mobile          bool                   `json:"mobile,omitempty"`
	Viewport        *driver.ViewportConfig `json:"viewport,omitempty"`
	Location        *driver.GeoLocation    `json:"location,omitempty"`
	Proxy           *proxyRequest          `json:"proxy,omitempty"`
	BlockAds        bool                   `json:"blockAds,omitempty"`
	MaxAge          int64                  `json:"maxAge,omitempty"`
	ParserMode      string                 `json:"parserMode,omitempty"`
	RenderJS        *bool                  `json:"renderJs,omitempty"`
	AutoScroll      bool                   `json:"autoScroll,omitempty"`
}

type v2FormatRequest struct {
	Type   string          `json:"type"`
	Schema json.RawMessage `json:"schema,omitempty"`
	Prompt string          `json:"prompt,omitempty"`
}

type v2CrawlCreateResponse struct {
	Success bool   `json:"success"`
	ID      string `json:"id"`
	JobID   string `json:"job_id"`
	URL     string `json:"url"`
	Status  string `json:"status"`
	Error   string `json:"error,omitempty"`
}

type v2CrawlStatusResponse struct {
	Success   bool                    `json:"success"`
	Status    string                  `json:"status"`
	Completed int                     `json:"completed"`
	Total     int                     `json:"total"`
	ExpiresAt time.Time               `json:"expiresAt"`
	Next      string                  `json:"next,omitempty"`
	Data      []*quarrycrawl.Document `json:"data"`
	Warning   string                  `json:"warning,omitempty"`
	Error     string                  `json:"error,omitempty"`
}

type v2CrawlErrorsResponse struct {
	Success       bool                     `json:"success"`
	Errors        []*quarrycrawl.PageError `json:"errors"`
	RobotsBlocked []string                 `json:"robotsBlocked"`
}

type previewPlanTarget struct {
	URL string `json:"url"`
}

func (h *Handler) registerV2Crawl(v2 fiber.Router) {
	v2.Post("/crawl/params-preview", h.v2CrawlParamsPreview)
	v2.Post("/crawl", h.v2Crawl)
	v2.Post("/map", h.v2Map)
	v2.Get("/crawl/:id/errors", h.v2CrawlErrors)
	v2.Get("/crawl/:id/documents", h.v2CrawlDocuments)
	v2.Get("/crawl/:id", h.v2CrawlStatus)
	v2.Delete("/crawl/:id", h.v2CancelCrawl)
}

func (h *Handler) v2Crawl(c *fiber.Ctx) error {
	if h.jobStore == nil || h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl executor is not initialized", nil)
	}

	req, spec, err := h.parseV2CrawlRequest(c, false)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if spec, err = h.applyPromptGeneratedCrawlOptions(c.UserContext(), c.Body(), req, spec); err != nil {
		return writeError(c, http.StatusBadGateway, "failed to generate crawl plan", err.Error())
	}

	if req.Mode != "" {
		zlog.Warn().Str("mode", req.Mode).Msg("v2 crawl ignores deprecated mode field")
	}

	meta := map[string]string{
		"kind":         "crawl",
		"url":          spec.URL,
		"module":       spec.Module,
		"mode":         "async",
		"api_version":  "v2",
		"crawl_style":  "recursive",
		"result_store": "crawl",
	}
	job := h.jobStore.New(meta)
	_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
		current.Status = jobs.StatusPending
		current.Progress = 0
	})

	run := &quarrycrawl.Run{
		ID:        job.ID,
		URL:       spec.URL,
		Status:    quarrycrawl.StatusQueued,
		CreatedAt: job.CreatedAt,
		UpdatedAt: job.UpdatedAt,
		ExpiresAt: job.ExpiresAt,
		Spec:      spec,
	}
	if err := h.crawlStore.CreateRun(context.Background(), run); err != nil {
		_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = err.Error()
		})
		return writeError(c, http.StatusInternalServerError, "failed to create crawl run", err.Error())
	}

	if h.sharedPublisher != nil {
		metaIface := make(map[string]interface{}, len(meta))
		for key, value := range meta {
			metaIface[key] = value
		}
		_ = h.sharedPublisher.PublishCrawlStarted(c.UserContext(), c.Get("X-Org-ID"), spec.URL, job.ID, metaIface)
	}

	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindCrawl, job.ID, asyncjobs.CrawlPayload{
		Spec:    spec,
		Webhook: req.Webhook,
		OrgID:   c.Get("X-Org-ID"),
		UserID:  c.Get("X-User-ID"),
	}, "v2"); err != nil {
		run.Status = quarrycrawl.StatusFailed
		run.UpdatedAt = time.Now().UTC()
		_ = h.crawlStore.SetRun(context.Background(), run)
		markJobDispatchFailure(h.jobStore, job.ID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue crawl job", err.Error())
	}

	return c.JSON(v2CrawlCreateResponse{
		Success: true,
		ID:      job.ID,
		JobID:   job.ID,
		URL:     h.buildCrawlStatusURL(c, job.ID),
		Status:  "scraping",
	})
}

func (h *Handler) v2CrawlStatus(c *fiber.Ctx) error {
	runID := strings.TrimSpace(c.Params("id"))
	if runID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl store is not initialized", nil)
	}

	run, err := h.crawlStore.GetRun(c.UserContext(), runID)
	if err != nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	skip := c.QueryInt("skip", 0)
	limit := c.QueryInt("limit", 100)
	if limit <= 0 {
		limit = 100
	}

	data, totalDocs, listErr := h.crawlStore.ListDocuments(c.UserContext(), runID, skip, limit)
	if listErr != nil {
		return writeError(c, http.StatusInternalServerError, "failed to list crawl documents", listErr.Error())
	}

	next := ""
	if skip+len(data) < totalDocs || run.Status == quarrycrawl.StatusQueued || run.Status == quarrycrawl.StatusRunning {
		next = h.buildCrawlStatusURL(c, runID)
		next += fmt.Sprintf("?skip=%d&limit=%d", skip+len(data), limit)
	}

	status := v2CrawlStatusResponse{
		Success:   run.Status != quarrycrawl.StatusFailed,
		Status:    mapRunStatus(run.Status),
		Completed: run.Completed,
		Total:     run.Total,
		ExpiresAt: run.ExpiresAt,
		Next:      next,
		Data:      data,
		Warning:   run.Warning,
	}
	if run.Status == quarrycrawl.StatusFailed {
		status.Error = "crawl failed"
	}
	return c.JSON(status)
}

func (h *Handler) v2CrawlErrors(c *fiber.Ctx) error {
	runID := strings.TrimSpace(c.Params("id"))
	if runID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl store is not initialized", nil)
	}

	if _, err := h.crawlStore.GetRun(c.UserContext(), runID); err != nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	pageErrors, err := h.crawlStore.ListErrors(c.UserContext(), runID)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to retrieve crawl errors", err.Error())
	}
	robotsBlocked, err := h.crawlStore.ListRobotsBlocked(c.UserContext(), runID)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to retrieve robots blocked urls", err.Error())
	}

	return c.JSON(v2CrawlErrorsResponse{
		Success:       true,
		Errors:        pageErrors,
		RobotsBlocked: robotsBlocked,
	})
}

// v2CrawlDocuments returns paginated documents from a crawl run.
// GET /v2/crawl/:id/documents?skip=0&limit=100
func (h *Handler) v2CrawlDocuments(c *fiber.Ctx) error {
	runID := strings.TrimSpace(c.Params("id"))
	if runID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl store is not initialized", nil)
	}

	if _, err := h.crawlStore.GetRun(c.UserContext(), runID); err != nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	skip := c.QueryInt("skip", 0)
	limit := c.QueryInt("limit", 100)
	if limit <= 0 {
		limit = 100
	}
	if limit > 1000 {
		limit = 1000
	}

	data, totalDocs, err := h.crawlStore.ListDocuments(c.UserContext(), runID, skip, limit)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to list documents", err.Error())
	}

	next := ""
	if skip+len(data) < totalDocs {
		next = h.buildCrawlStatusURL(c, runID) + fmt.Sprintf("/documents?skip=%d&limit=%d", skip+len(data), limit)
	}

	return c.JSON(fiber.Map{
		"success": true,
		"total":   totalDocs,
		"skip":    skip,
		"limit":   limit,
		"next":    next,
		"data":    data,
	})
}

func (h *Handler) v2CancelCrawl(c *fiber.Ctx) error {
	runID := strings.TrimSpace(c.Params("id"))
	if runID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil || h.crawlStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "crawl store is not initialized", nil)
	}

	run, err := h.crawlStore.GetRun(c.UserContext(), runID)
	if err != nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	run.Status = quarrycrawl.StatusCancelled
	run.Active = 0
	run.Queued = 0
	run.UpdatedAt = time.Now().UTC()
	if err := h.crawlStore.SetRun(c.UserContext(), run); err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to update crawl status", err.Error())
	}

	if cancelValue, ok := h.activeCrawlCancels.Load(runID); ok {
		if cancelFn, ok := cancelValue.(context.CancelFunc); ok {
			cancelFn()
		}
	}
	h.publishAsyncCancel(c.UserContext(), asyncjobs.KindCrawl, runID)

	_, _ = h.jobStore.Update(runID, func(current *jobs.Job) {
		current.Status = jobs.StatusCancelled
		current.Error = "cancelled by user"
	})

	return c.JSON(fiber.Map{
		"success": true,
		"id":      runID,
		"job_id":  runID,
		"status":  "cancelled",
	})
}

func (h *Handler) v2Map(c *fiber.Ctx) error {
	req, spec, err := h.parseV2CrawlRequest(c, true)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	_ = req

	tempStore := quarrycrawl.NewMemoryStore(time.Minute)
	run := &quarrycrawl.Run{
		ID:        "map",
		URL:       spec.URL,
		Status:    quarrycrawl.StatusQueued,
		CreatedAt: time.Now().UTC(),
		UpdatedAt: time.Now().UTC(),
		ExpiresAt: time.Now().Add(time.Minute),
		Spec:      spec,
	}
	if err := tempStore.CreateRun(c.UserContext(), run); err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to initialize map run", err.Error())
	}

	if err := quarrycrawl.Execute(c.UserContext(), tempStore, run, spec, h.fetchCrawlPage); err != nil {
		if errors.Is(err, context.Canceled) {
			return writeError(c, http.StatusRequestTimeout, "map request cancelled", nil)
		}
		return writeError(c, http.StatusBadGateway, "map failed", err.Error())
	}

	docs, _, err := tempStore.ListDocuments(c.UserContext(), run.ID, 0, spec.Limit)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to load map results", err.Error())
	}

	links := make([]string, 0, len(docs))
	for _, doc := range docs {
		links = append(links, doc.URL)
	}
	sort.Strings(links)

	return c.JSON(fiber.Map{
		"success": true,
		"url":     spec.URL,
		"count":   len(links),
		"links":   links,
	})
}

func (h *Handler) v2CrawlParamsPreview(c *fiber.Ctx) error {
	req, spec, err := h.parseV2CrawlRequest(c, true)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if strings.TrimSpace(req.Prompt) == "" {
		return writeError(c, http.StatusBadRequest, "prompt is required", nil)
	}

	sample, err := h.sampleCrawlURLs(c.UserContext(), spec)
	if err != nil {
		return writeError(c, http.StatusBadGateway, "failed to sample site structure", err.Error())
	}

	data, err := h.previewCrawlOptions(c.UserContext(), spec, sample)
	if err != nil {
		return writeError(c, http.StatusBadGateway, "failed to generate crawl params preview", err.Error())
	}

	return c.JSON(fiber.Map{
		"success": true,
		"data":    data,
	})
}

func (h *Handler) runV2CrawlJob(ctx context.Context, runID string, spec quarrycrawl.Spec, webhook *models.WebhookConfig, orgID, userID string) {
	defer h.activeCrawlCancels.Delete(runID)
	// Forward orgID into spec so per-page agent-assist has billing context.
	if spec.OrgID == "" {
		spec.OrgID = orgID
	}
	eventURL := spec.URL
	if spec.ZDRMode {
		eventURL = ""
	}

	run, err := h.crawlStore.GetRun(context.Background(), runID)
	if err != nil {
		_, _ = h.jobStore.Update(runID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = err.Error()
		})
		return
	}

	err = quarrycrawl.Execute(ctx, h.crawlStore, run, spec, h.fetchCrawlPage)
	updatedRun, getErr := h.crawlStore.GetRun(context.Background(), runID)
	if getErr != nil {
		updatedRun = run
	}

	switch {
	case errors.Is(err, context.Canceled) || updatedRun.Status == quarrycrawl.StatusCancelled:
		_, _ = h.jobStore.Update(runID, func(current *jobs.Job) {
			current.Status = jobs.StatusCancelled
			current.Error = "cancelled by user"
			current.Progress = 0
		})
		if h.streamManager != nil {
			h.streamManager.Broadcast(runID, sse.EventJobFailed, map[string]any{"status": "cancelled"})
		}
		return
	case err != nil:
		updatedRun.Status = quarrycrawl.StatusFailed
		updatedRun.UpdatedAt = time.Now().UTC()
		_ = h.crawlStore.SetRun(context.Background(), updatedRun)
		_, _ = h.jobStore.Update(runID, func(current *jobs.Job) {
			current.Status = jobs.StatusFailed
			current.Error = err.Error()
		})
		if h.sharedPublisher != nil {
			_ = h.sharedPublisher.PublishCrawlFailed(context.Background(), orgID, eventURL, runID, err.Error(), map[string]interface{}{
				"api_version": "v2",
			})
		}
		if webhook != nil && webhook.URL != "" && shouldSendPlatformWebhookEvent(webhook.Events, "crawl.failed") {
			go h.fireWebhook(webhook.URL, &models.WebhookPayload{
				Success: false,
				Type:    "crawl.failed",
				ID:      runID,
				Error:   err.Error(),
			})
		}
		if h.streamManager != nil {
			h.streamManager.Broadcast(runID, sse.EventJobFailed, map[string]any{"error": err.Error()})
		}
		return
	default:
		_, _ = h.jobStore.Update(runID, func(current *jobs.Job) {
			current.Status = jobs.StatusReady
			current.Progress = 100
			current.Result = map[string]any{
				"completed": updatedRun.Completed,
				"total":     updatedRun.Total,
			}
		})
		if h.sharedPublisher != nil {
			_ = h.sharedPublisher.PublishCrawlCompleted(context.Background(), orgID, eventURL, runID, updatedRun.Completed, map[string]interface{}{
				"api_version": "v2",
			})
			if userID != "" {
				_ = h.sharedPublisher.PublishNotificationCrawlCompleted(context.Background(), userID, runID, eventURL, orgID, updatedRun.Completed)
			}
		}
		if webhook != nil && webhook.URL != "" && shouldSendPlatformWebhookEvent(webhook.Events, "crawl.completed") {
			metadata := h.buildCrawlWebhookMetadata(context.Background(), updatedRun)
			go h.fireWebhook(webhook.URL, &models.WebhookPayload{
				Success:  true,
				Type:     "crawl.completed",
				ID:       runID,
				Metadata: metadata,
			})
		}
		if h.streamManager != nil {
			h.streamManager.Broadcast(runID, sse.EventJobCompleted, map[string]any{"progress": 100})
		}
		// Background: push crawled pages to the data plane for embedding/retrieval.
		if h.dataplaneClient != nil && orgID != "" {
			go h.ingestCrawlDocumentsToDataPlane(context.Background(), runID, orgID, eventURL)
		}
	}
}

// ingestCrawlDocumentsToDataPlane pushes all crawled pages to the data plane for embedding.
// Runs as a background goroutine after a successful crawl completion without blocking
// the NATS/SSE completion events. Non-fatal: logs errors but never panics.
func (h *Handler) ingestCrawlDocumentsToDataPlane(ctx context.Context, runID, orgID, baseURL string) {
	if h.dataplaneClient == nil || orgID == "" {
		return
	}

	const batchSize = 20
	skip := 0
	ingested := 0
	failed := 0

	for {
		if ctx.Err() != nil {
			return
		}

		docs, total, err := h.crawlStore.ListDocuments(ctx, runID, skip, batchSize)
		if err != nil {
			zlog.Error().Err(err).Str("run_id", runID).Msg("crawl→dataplane: failed to list documents")
			return
		}
		if len(docs) == 0 {
			break
		}

		for _, doc := range docs {
			if ctx.Err() != nil {
				return
			}

			content := crawlDocContent(doc.Outputs)
			if content == "" {
				continue
			}

			docReq := &dataplane.DocumentCreateRequest{
				OrgID:   orgID,
				Source:  "quarry",
				Type:    "crawl",
				Title:   crawlDocTitle(doc.Metadata, doc.URL),
				Content: content,
				Metadata: map[string]interface{}{
					"crawl_id":   runID,
					"url":        doc.URL,
					"source_url": baseURL,
				},
			}

			if _, err := h.ingestExtraction(ctx, orgID, docReq); err != nil {
				zlog.Warn().Err(err).Str("url", doc.URL).Msg("crawl→dataplane: ingest error")
				failed++
			} else {
				ingested++
			}

			// Publish live progress every 10 docs.
			if h.sharedPublisher != nil && (ingested+failed)%10 == 0 && total > 0 {
				pct := (ingested + failed) * 100 / total
				_ = h.sharedPublisher.PublishJobProgress(ctx, orgID, runID, pct, ingested, total, "indexing pages")
			}
		}

		skip += len(docs)
		if skip >= total {
			break
		}
	}

	zlog.Info().
		Str("run_id", runID).
		Int("ingested", ingested).
		Int("failed", failed).
		Msg("crawl→dataplane: ingestion complete")

	// Signal to Convex that data-plane ingestion is done → job transitions to "completed".
	if h.sharedPublisher != nil {
		_ = h.sharedPublisher.PublishCrawlIndexed(ctx, orgID, runID, ingested)
	}
}

// crawlDocContent extracts the best available text from a crawled page's Outputs.
func crawlDocContent(outputs map[string]interface{}) string {
	for _, key := range []string{"markdown", "rawMarkdown", "text", "content", "html"} {
		if v, ok := outputs[key]; ok {
			if s, ok := v.(string); ok && strings.TrimSpace(s) != "" {
				return s
			}
		}
	}
	return ""
}

// crawlDocTitle extracts the page title from crawl metadata, falling back to the URL.
func crawlDocTitle(metadata map[string]interface{}, fallbackURL string) string {
	for _, key := range []string{"title", "ogTitle", "og:title"} {
		if v, ok := metadata[key]; ok {
			if s, ok := v.(string); ok && strings.TrimSpace(s) != "" {
				return s
			}
		}
	}
	if len(fallbackURL) > 80 {
		return fallbackURL[:80] + "..."
	}
	return fallbackURL
}

func (h *Handler) fetchCrawlPage(ctx context.Context, item quarrycrawl.Item, spec quarrycrawl.Spec) (*quarrycrawl.FetchedPage, error) {
	if h == nil {
		return nil, fmt.Errorf("handler is nil")
	}

	pageOptions := spec.PageOptions
	formats, structuredFormat := selectCrawlFormats(pageOptions.Formats)
	requestedJSON := containsFormatName(formats, "json")
	needsLinks := !containsFormatName(formats, "links")
	requestedLinks := containsFormatName(formats, "links")
	if spec.DiscoveryOnly {
		formats = []string{"links"}
		needsLinks = false
		requestedLinks = true
	} else if needsLinks {
		formats = append(formats, "links")
	}
	if !spec.DiscoveryOnly && !requestedJSON {
		formats = append(formats, "json")
	}

	outputs, actionResults, err := h.fetchFormats(ctx, item.URL, &scraper.FormatOptions{
		Formats:         formats,
		IncludeTags:     pageOptions.IncludeTags,
		ExcludeTags:     pageOptions.ExcludeTags,
		OnlyMainContent: pageOptions.OnlyMainContent,
		WaitFor:         pageOptions.WaitFor,
		MaxAgeMs:        pageOptions.MaxAgeMs,
		Headers:         pageOptions.Headers,
		Actions:         pageOptions.Actions,
		Mobile:          pageOptions.Mobile,
		Viewport:        pageOptions.Viewport,
		Location:        pageOptions.Location,
		ProxyURL:        pageOptions.ProxyURL,
		BlockAds:        pageOptions.BlockAds,
		ParserMode:      pageOptions.ParserMode,
		RenderJS:        pageOptions.RenderJS,
		AutoScroll:      pageOptions.AutoScroll,
		// Agent-assist: enable AI-driven interaction when the crawl specifies a prompt
		// and the rendered page has insufficient content.
		AgentGoal:   spec.Prompt,
		AgentSchema: spec.Schema,
		OrgID:       spec.OrgID,
	})
	if err != nil {
		return nil, err
	}
	if outputs == nil {
		outputs = map[string]interface{}{}
	}

	links := extractLinks(outputs["links"])
	statusCode, contentType := extractFetchMetadata(outputs["json"])
	if !requestedLinks {
		delete(outputs, "links")
	}
	if !requestedJSON {
		delete(outputs, "json")
	}

	if structuredFormat != nil {
		extracted, extractErr := h.extractStructured(ctx, item.URL, &scraper.StructuredExtractOptions{
			Schema:   structuredFormat.Schema,
			Prompt:   structuredFormat.Prompt,
			MaxAgeMs: pageOptions.MaxAgeMs,
		})
		if extractErr != nil {
			return nil, extractErr
		}
		outputs["json"] = extracted
	}

	persistedOutputs := outputs
	persistedActions := toActionExecutionResults(actionResults)
	if !spec.ZDRMode {
		persistedOutputs, persistedActions = h.persistResponseArtifacts(ctx, item.URL, outputs, persistedActions)
	}
	if len(persistedActions) > 0 {
		persistedOutputs["actions"] = persistedActions
	}

	if spec.DiscoveryOnly {
		persistedOutputs = nil
	}

	metadata := map[string]interface{}{
		"depth":           item.Depth,
		"sourceURL":       item.SourceURL,
		"httpStatus":      statusCode,
		"contentType":     contentType,
		"status":          "completed",
		"duplicateReason": "",
		"blockedReason":   "",
		"pageStatus": map[string]interface{}{
			"httpStatus":      statusCode,
			"contentType":     contentType,
			"depth":           item.Depth,
			"sourceURL":       item.SourceURL,
			"status":          "completed",
			"duplicateReason": "",
			"blockedReason":   "",
		},
	}
	if h.changeTracker != nil && spec.ChangeTracking != nil && spec.ChangeTracking.Enabled && !spec.DiscoveryOnly {
		payload, payloadErr := buildChangeTrackingPayload(nil, outputs)
		if payloadErr == nil && strings.TrimSpace(payload) != "" {
			var (
				changeResult *models.ChangeTrackingResult
				trackErr     error
			)
			if spec.ChangeTracking.DryRun {
				changeResult, trackErr = h.changeTracker.Compare(ctx, item.URL, payload, spec.ChangeTracking)
			} else {
				changeResult, trackErr = h.changeTracker.Track(ctx, item.URL, payload, spec.ChangeTracking)
			}
			if trackErr == nil && changeResult != nil {
				metadata["changeTracking"] = changeResult
			} else if trackErr != nil {
				metadata["changeTrackingError"] = trackErr.Error()
			}
		}
	}

	page := &quarrycrawl.FetchedPage{
		URL:         item.URL,
		StatusCode:  statusCode,
		ContentType: contentType,
		Links:       links,
		Outputs:     persistedOutputs,
		Metadata:    metadata,
	}
	if spec.ZDRMode {
		page = sanitizeCrawlFetchedPageForZDR(page)
	}
	return page, nil
}

func (h *Handler) previewCrawlOptions(ctx context.Context, spec quarrycrawl.Spec, sample []string) (map[string]interface{}, error) {
	if h.crawlPreviewFn != nil {
		return h.crawlPreviewFn(ctx, spec, sample)
	}

	base := heuristicPreview(spec, sample)
	if h.scraper == nil || h.scraper.AIClient() == nil || strings.TrimSpace(spec.Prompt) == "" {
		return base, nil
	}

	planResp, err := h.scraper.AIClient().PlanCrawl(ctx, &ai.PlanRequest{
		URL:      spec.URL,
		MaxDepth: 3,
		Prompt:   buildPreviewPrompt(spec.Prompt, sample),
	})
	if err != nil {
		return base, nil
	}

	merged := mergePreview(base, previewFromPlan(spec, planResp.Plan))
	return merged, nil
}

func (h *Handler) sampleCrawlURLs(ctx context.Context, spec quarrycrawl.Spec) ([]string, error) {
	tempStore := quarrycrawl.NewMemoryStore(time.Minute)
	sampleSpec := spec
	sampleSpec.DiscoveryOnly = true
	sampleSpec.Limit = min(spec.Limit, 50)
	if sampleSpec.Limit <= 0 {
		sampleSpec.Limit = 50
	}

	run := &quarrycrawl.Run{
		ID:        "preview",
		URL:       sampleSpec.URL,
		Status:    quarrycrawl.StatusQueued,
		CreatedAt: time.Now().UTC(),
		UpdatedAt: time.Now().UTC(),
		ExpiresAt: time.Now().Add(time.Minute),
		Spec:      sampleSpec,
	}
	if err := tempStore.CreateRun(ctx, run); err != nil {
		return nil, err
	}
	if err := quarrycrawl.Execute(ctx, tempStore, run, sampleSpec, h.fetchCrawlPage); err != nil {
		return nil, err
	}
	docs, _, err := tempStore.ListDocuments(ctx, run.ID, 0, sampleSpec.Limit)
	if err != nil {
		return nil, err
	}
	sample := make([]string, 0, len(docs))
	for _, doc := range docs {
		sample = append(sample, doc.URL)
	}
	return sample, nil
}

func (h *Handler) applyPromptGeneratedCrawlOptions(ctx context.Context, body []byte, req *v2CrawlRequest, spec quarrycrawl.Spec) (quarrycrawl.Spec, error) {
	if req == nil || strings.TrimSpace(req.Prompt) == "" {
		return spec, nil
	}

	sampleSpec := spec
	sampleSpec.IncludePaths = nil
	sampleSpec.ExcludePaths = nil
	sample, err := h.sampleCrawlURLs(ctx, sampleSpec)
	if err != nil {
		return spec, err
	}

	preview, err := h.previewCrawlOptions(ctx, sampleSpec, sample)
	if err != nil {
		return spec, err
	}
	return mergePromptCrawlSpec(spec, preview, fieldPresence(body)), nil
}

func mergePromptCrawlSpec(spec quarrycrawl.Spec, preview map[string]interface{}, present map[string]struct{}) quarrycrawl.Spec {
	if len(preview) == 0 {
		return spec
	}

	if _, ok := present["limit"]; !ok {
		if limit := intFromAny(preview["limit"]); limit > 0 {
			spec.Limit = limit
		}
	}
	if _, ok := present["includePaths"]; !ok {
		if include := stringSliceFromAny(preview["includePaths"]); len(include) > 0 {
			spec.IncludePaths = include
		}
	}
	if _, ok := present["excludePaths"]; !ok {
		if exclude := stringSliceFromAny(preview["excludePaths"]); len(exclude) > 0 {
			spec.ExcludePaths = exclude
		}
	}
	if _, ok := present["crawlEntireDomain"]; !ok {
		if crawlEntireDomain, ok := preview["crawlEntireDomain"].(bool); ok {
			spec.CrawlEntireDomain = crawlEntireDomain
		}
	}
	if _, ok := present["allowExternalLinks"]; !ok {
		if allowExternalLinks, ok := preview["allowExternalLinks"].(bool); ok {
			spec.AllowExternalLinks = allowExternalLinks
		}
	}
	if _, ok := present["allowSubdomains"]; !ok {
		if allowSubdomains, ok := preview["allowSubdomains"].(bool); ok {
			spec.AllowSubdomains = allowSubdomains
		}
	}
	if _, ok := present["ignoreQueryParameters"]; !ok {
		if ignoreQueryParameters, ok := preview["ignoreQueryParameters"].(bool); ok {
			spec.IgnoreQueryParameters = ignoreQueryParameters
		}
	}
	if _, ok := present["sitemap"]; !ok {
		if sitemap := strings.TrimSpace(stringFromAny(preview["sitemap"], "")); sitemap != "" {
			spec.Sitemap = quarrycrawl.SitemapMode(sitemap)
		}
	}
	return spec
}

func (h *Handler) parseV2CrawlRequest(c *fiber.Ctx, discoveryOnly bool) (*v2CrawlRequest, quarrycrawl.Spec, error) {
	var req v2CrawlRequest
	if err := json.Unmarshal(c.Body(), &req); err != nil {
		return nil, quarrycrawl.Spec{}, fmt.Errorf("invalid JSON body")
	}

	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return nil, quarrycrawl.Spec{}, err
	}
	if req.Webhook != nil && req.Webhook.URL != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return nil, quarrycrawl.Spec{}, fmt.Errorf("webhook url is invalid: %w", err)
		}
	}
	if req.MaxDiscoveryDepth != nil && *req.MaxDiscoveryDepth > v2CrawlMaxDepth {
		return nil, quarrycrawl.Spec{}, fmt.Errorf("maxDiscoveryDepth exceeds limit (%d)", v2CrawlMaxDepth)
	}
	if req.MaxDepth != nil && *req.MaxDepth > v2CrawlMaxDepth {
		return nil, quarrycrawl.Spec{}, fmt.Errorf("maxDepth exceeds limit (%d)", v2CrawlMaxDepth)
	}
	if req.Limit > v2CrawlMaxLimit {
		return nil, quarrycrawl.Spec{}, fmt.Errorf("limit exceeds limit (%d)", v2CrawlMaxLimit)
	}
	if req.Module != "" && !modulePattern.MatchString(req.Module) {
		return nil, quarrycrawl.Spec{}, fmt.Errorf("module contains invalid characters")
	}

	pageOptions, err := parseV2ScrapeOptions(req.ScrapeOptions)
	if err != nil {
		return nil, quarrycrawl.Spec{}, err
	}
	if len(pageOptions.Headers) == 0 && len(req.Headers) > 0 {
		pageOptions.Headers = cloneStringMap(req.Headers)
	}
	if req.MaxAge > 0 && pageOptions.MaxAgeMs == 0 {
		pageOptions.MaxAgeMs = req.MaxAge
	}
	if req.ScrapeOptions != nil {
		pageOptions.ProxyURL = h.resolveProxyURL(c, req.ScrapeOptions.Proxy)
	}

	schema, err := normalizeOptionalSchema(req.Schema)
	if err != nil {
		return nil, quarrycrawl.Spec{}, fmt.Errorf("schema must be valid JSON or a JSON-encoded string")
	}

	spec, err := quarrycrawl.NormalizeSpec(quarrycrawl.NormalizeInput{
		URL:                    req.URL,
		Preset:                 req.Preset,
		IncludePaths:           req.IncludePaths,
		ExcludePaths:           req.ExcludePaths,
		MaxDiscoveryDepth:      req.MaxDiscoveryDepth,
		MaxDepth:               req.MaxDepth,
		Limit:                  req.Limit,
		CrawlEntireDomain:      req.CrawlEntireDomain,
		AllowExternalLinks:     req.AllowExternalLinks,
		AllowSubdomains:        req.AllowSubdomains,
		IgnoreRobotsTxt:        req.IgnoreRobotsTxt,
		Sitemap:                req.Sitemap,
		DeduplicateSimilarURLs: req.DeduplicateSimilarURLs,
		IgnoreQueryParameters:  req.IgnoreQueryParameters,
		RegexOnFullURL:         req.RegexOnFullURL,
		RegexPaths:             req.RegexPaths,
		DelayMs:                req.Delay,
		MaxConcurrency:         req.MaxConcurrency,
		Prompt:                 req.Prompt,
		Schema:                 schema,
		Module:                 req.Module,
		Enrich:                 req.Enrich,
		EnrichLimit:            req.EnrichLimit,
		MaxAge:                 req.MaxAge,
		ScheduleAt:             req.ScheduleAt,
		ChangeTracking:         req.ChangeTracking,
		PageOptions:            pageOptions,
		DiscoveryOnly:          discoveryOnly,
	})
	if err != nil {
		return nil, quarrycrawl.Spec{}, err
	}
	if _, err := quarrycrawl.NewMatcher(spec); err != nil {
		return nil, quarrycrawl.Spec{}, err
	}

	return &req, spec, nil
}

func parseV2ScrapeOptions(raw *v2ScrapeOptionsRequest) (quarrycrawl.PageOptions, error) {
	if raw == nil {
		return quarrycrawl.PageOptions{}, nil
	}
	formats, err := parseV2Formats(raw.Formats)
	if err != nil {
		return quarrycrawl.PageOptions{}, err
	}
	proxyURL := ""
	if raw.Proxy != nil {
		proxyURL = strings.TrimSpace(raw.Proxy.URL)
	}
	return quarrycrawl.PageOptions{
		Formats:         formats,
		Headers:         cloneStringMap(raw.Headers),
		WaitFor:         raw.WaitFor,
		OnlyMainContent: raw.OnlyMainContent,
		IncludeTags:     append([]string(nil), raw.IncludeTags...),
		ExcludeTags:     append([]string(nil), raw.ExcludeTags...),
		Actions:         toActionSteps(raw.Actions),
		Mobile:          raw.Mobile,
		Viewport:        raw.Viewport,
		Location:        raw.Location,
		ProxyURL:        proxyURL,
		BlockAds:        raw.BlockAds,
		MaxAgeMs:        raw.MaxAge,
		ParserMode:      strings.ToLower(strings.TrimSpace(raw.ParserMode)),
		RenderJS:        raw.RenderJS,
		AutoScroll:      raw.AutoScroll,
	}, nil
}

func parseV2Formats(raw []json.RawMessage) ([]quarrycrawl.Format, error) {
	if len(raw) == 0 {
		return nil, nil
	}
	formats := make([]quarrycrawl.Format, 0, len(raw))
	for _, item := range raw {
		trimmed := strings.TrimSpace(string(item))
		if trimmed == "" || trimmed == "null" {
			continue
		}

		var formatName string
		if err := json.Unmarshal(item, &formatName); err == nil {
			formats = append(formats, quarrycrawl.Format{Type: strings.TrimSpace(formatName)})
			continue
		}

		var formatObj v2FormatRequest
		if err := json.Unmarshal(item, &formatObj); err != nil {
			return nil, fmt.Errorf("invalid scrapeOptions.formats item")
		}
		schema, err := normalizeOptionalSchema(formatObj.Schema)
		if err != nil {
			return nil, fmt.Errorf("invalid scrapeOptions.formats schema")
		}
		formats = append(formats, quarrycrawl.Format{
			Type:   strings.TrimSpace(formatObj.Type),
			Schema: schema,
			Prompt: strings.TrimSpace(formatObj.Prompt),
		})
	}
	return formats, nil
}

func normalizeOptionalSchema(raw json.RawMessage) (string, error) {
	trimmed := strings.TrimSpace(string(raw))
	if trimmed == "" || trimmed == "null" {
		return "", nil
	}
	var schemaString string
	if err := json.Unmarshal(raw, &schemaString); err == nil {
		return CompactJSON(schemaString)
	}
	return CompactJSON(trimmed)
}

func selectCrawlFormats(formats []quarrycrawl.Format) ([]string, *quarrycrawl.Format) {
	rawFormats := make([]string, 0, len(formats))
	seen := make(map[string]struct{}, len(formats))
	var structuredFormat *quarrycrawl.Format

	for _, format := range formats {
		formatType := strings.ToLower(strings.TrimSpace(format.Type))
		if formatType == "" {
			continue
		}
		if formatType == "json" && (strings.TrimSpace(format.Schema) != "" || strings.TrimSpace(format.Prompt) != "") {
			copied := format
			structuredFormat = &copied
			continue
		}
		if _, ok := seen[formatType]; ok {
			continue
		}
		seen[formatType] = struct{}{}
		rawFormats = append(rawFormats, formatType)
	}

	return rawFormats, structuredFormat
}

func containsFormatName(formats []string, expected string) bool {
	for _, format := range formats {
		if strings.EqualFold(strings.TrimSpace(format), expected) {
			return true
		}
	}
	return false
}

func extractLinks(value interface{}) []string {
	switch typed := value.(type) {
	case []string:
		return append([]string(nil), typed...)
	case []interface{}:
		links := make([]string, 0, len(typed))
		for _, item := range typed {
			asString, ok := item.(string)
			if ok && strings.TrimSpace(asString) != "" {
				links = append(links, asString)
			}
		}
		return links
	default:
		return nil
	}
}

func mapRunStatus(status quarrycrawl.RunStatus) string {
	switch status {
	case quarrycrawl.StatusQueued, quarrycrawl.StatusRunning:
		return "scraping"
	case quarrycrawl.StatusCompleted:
		return "completed"
	case quarrycrawl.StatusCancelled:
		return "cancelled"
	case quarrycrawl.StatusFailed:
		return "failed"
	default:
		return "scraping"
	}
}

func (h *Handler) buildCrawlStatusURL(c *fiber.Ctx, runID string) string {
	host := strings.TrimSpace(c.Get("Host"))
	if host == "" {
		host = c.Hostname()
	}
	return fmt.Sprintf("%s://%s/v2/crawl/%s", c.Protocol(), host, runID)
}

func heuristicPreview(spec quarrycrawl.Spec, sample []string) map[string]interface{} {
	preview := map[string]interface{}{
		"url":                   spec.URL,
		"limit":                 min(max(len(sample)*2, 100), 1000),
		"allowExternalLinks":    spec.AllowExternalLinks,
		"allowSubdomains":       spec.AllowSubdomains,
		"crawlEntireDomain":     spec.CrawlEntireDomain,
		"ignoreQueryParameters": spec.IgnoreQueryParameters,
		"sitemap":               spec.Sitemap,
	}
	if preview["limit"].(int) == 0 {
		preview["limit"] = 100
	}
	if len(spec.IncludePaths) > 0 {
		preview["includePaths"] = append([]string(nil), spec.IncludePaths...)
	}
	if len(spec.ExcludePaths) > 0 {
		preview["excludePaths"] = append([]string(nil), spec.ExcludePaths...)
	}

	lowerPrompt := strings.ToLower(strings.TrimSpace(spec.Prompt))
	switch {
	case strings.Contains(lowerPrompt, "pricing"):
		preview["includePaths"] = []string{"/pricing/**", "/plans/**"}
	case strings.Contains(lowerPrompt, "docs"), strings.Contains(lowerPrompt, "documentation"):
		preview["includePaths"] = []string{"/docs/**", "/documentation/**"}
	case strings.Contains(lowerPrompt, "blog"), strings.Contains(lowerPrompt, "articles"):
		preview["includePaths"] = []string{"/blog/**", "/articles/**"}
	case strings.Contains(lowerPrompt, "entire"), strings.Contains(lowerPrompt, "whole site"), strings.Contains(lowerPrompt, "everything"):
		preview["crawlEntireDomain"] = true
	}
	return preview
}

func buildPreviewPrompt(prompt string, sample []string) string {
	if len(sample) == 0 {
		return prompt
	}
	var builder strings.Builder
	builder.WriteString(strings.TrimSpace(prompt))
	builder.WriteString("\n\nKnown site URLs:\n")
	for _, item := range sample {
		builder.WriteString("- ")
		builder.WriteString(item)
		builder.WriteByte('\n')
	}
	return builder.String()
}

func previewFromPlan(spec quarrycrawl.Spec, planJSON string) map[string]interface{} {
	type crawlPlan struct {
		Targets        []previewPlanTarget `json:"targets"`
		EstimatedPages int                 `json:"estimatedPages"`
	}

	var plan crawlPlan
	if err := json.Unmarshal([]byte(planJSON), &plan); err != nil {
		return nil
	}

	includePaths := deriveIncludePaths(spec.URL, plan.Targets)
	preview := map[string]interface{}{
		"url":   spec.URL,
		"limit": max(plan.EstimatedPages, 100),
	}
	if len(includePaths) > 0 {
		preview["includePaths"] = includePaths
	}
	return preview
}

func mergePreview(base, override map[string]interface{}) map[string]interface{} {
	if len(override) == 0 {
		return base
	}
	merged := make(map[string]interface{}, len(base)+len(override))
	for key, value := range base {
		merged[key] = value
	}
	for key, value := range override {
		merged[key] = value
	}
	return merged
}

func deriveIncludePaths(baseURL string, targets []previewPlanTarget) []string {
	start, err := url.Parse(baseURL)
	if err != nil {
		return nil
	}

	seen := make(map[string]struct{})
	paths := make([]string, 0, len(targets))
	for _, target := range targets {
		parsed, err := url.Parse(strings.TrimSpace(target.URL))
		if err != nil || parsed.Hostname() != start.Hostname() {
			continue
		}
		pathValue := strings.TrimSuffix(parsed.Path, "/")
		if pathValue == "" || pathValue == "/" {
			continue
		}
		firstSegment := strings.Split(strings.TrimPrefix(pathValue, "/"), "/")[0]
		if firstSegment == "" {
			continue
		}
		pattern := "/" + firstSegment + "/**"
		if _, ok := seen[pattern]; ok {
			continue
		}
		seen[pattern] = struct{}{}
		paths = append(paths, pattern)
	}
	sort.Strings(paths)
	if len(paths) > 5 {
		paths = paths[:5]
	}
	return paths
}

func cloneStringMap(src map[string]string) map[string]string {
	if len(src) == 0 {
		return nil
	}
	out := make(map[string]string, len(src))
	for key, value := range src {
		out[key] = value
	}
	return out
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
