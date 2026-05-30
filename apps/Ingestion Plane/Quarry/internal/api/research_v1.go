package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/gofiber/fiber/v2"
	"golang.org/x/sync/errgroup"

	"github.com/triodelab/quarry/internal/asyncjobs"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
	"github.com/triodelab/quarry/internal/sse"
)

type v1ResearchRequest struct {
	Query         string                  `json:"query"`
	Prompt        string                  `json:"prompt,omitempty"`
	SystemPrompt  string                  `json:"systemPrompt,omitempty"`
	Preset        string                  `json:"preset,omitempty"`
	BlendMode     string                  `json:"blendMode,omitempty"`
	Limit         int                     `json:"limit,omitempty"`
	MaxIterations int                     `json:"maxIterations,omitempty"`
	Sources       []json.RawMessage       `json:"sources,omitempty"`
	ScrapeOptions *v2ScrapeOptionsRequest `json:"scrapeOptions,omitempty"`
	Webhook       *models.WebhookConfig   `json:"webhook,omitempty"`
	TimeoutSec    int                     `json:"timeout,omitempty"`
}

type researchStep struct {
	Name   string `json:"name"`
	Status string `json:"status"`
	Detail string `json:"detail,omitempty"`
	Count  int    `json:"count,omitempty"`
}

type researchSource struct {
	Title   string `json:"title"`
	URL     string `json:"url"`
	Snippet string `json:"snippet,omitempty"`
	Source  string `json:"source,omitempty"`
	Type    string `json:"type,omitempty"`
	Content string `json:"content,omitempty"`
}

type researchStatusEnvelope struct {
	resourceStatusEnvelope
	Query   string           `json:"query"`
	Steps   []researchStep   `json:"steps"`
	Sources []researchSource `json:"sources"`
	Report  string           `json:"report,omitempty"`
	Data    map[string]any   `json:"data,omitempty"`
}

type v1ResearchRunConfig struct {
	OrgID         string
	Query         string
	Prompt        string
	SystemPrompt  string
	Preset        string
	BlendMode     string
	Limit         int
	MaxIterations int
	Sources       []v2SearchSource
	ScrapeOpts    *scraper.FormatOptions
	FocusTerms    []string
	Webhook       *models.WebhookConfig
	TimeoutSec    int
}

func (h *Handler) v1CancelExtract(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.extractionJobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "extraction store not initialized", nil)
	}

	if _, err := h.extractionJobStore.Get(c.UserContext(), jobID); err != nil {
		return writeError(c, http.StatusNotFound, "extraction job not found", nil)
	}
	if cancelValue, ok := h.activeExtractCancels.Load(jobID); ok {
		if cancelFn, ok := cancelValue.(context.CancelFunc); ok {
			cancelFn()
		}
	}
	h.publishAsyncCancel(c.UserContext(), asyncjobs.KindExtract, jobID)
	h.updateExtractLifecycle(context.Background(), jobID, jobs.ExtractionCancelled, 0, 0, nil, "cancelled by user")
	h.recordUserActivity(c, "extract.cancelled", "extract", map[string]interface{}{
		"jobId":   jobID,
		"summary": "extract cancelled",
	})
	return c.JSON(fiber.Map{"success": true, "id": jobID, "status": "cancelled"})
}

func (h *Handler) v1Research(c *fiber.Ctx) error {
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "research is not initialized", nil)
	}
	orgID, userID, tier := h.currentOrgAndUser(c)

	normalized, err := mergeTopLevelFetchAliases(c.Body())
	if err != nil {
		return writeError(c, http.StatusBadRequest, "invalid JSON body", nil)
	}
	c.Request().SetBodyRaw(normalized)

	var req v1ResearchRequest
	if err := json.Unmarshal(normalized, &req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	req.Query = strings.TrimSpace(req.Query)
	req.Prompt = strings.TrimSpace(req.Prompt)
	req.SystemPrompt = strings.TrimSpace(req.SystemPrompt)
	req.Preset = normalizeResearchPresetName(req.Preset)
	req.BlendMode = normalizeBlendMode(req.BlendMode)
	if req.BlendMode == "" {
		return writeError(c, http.StatusBadRequest, "blendMode contains unsupported value", nil)
	}
	if req.Query == "" {
		req.Query = req.Prompt
	}
	if req.Query == "" {
		return writeError(c, http.StatusBadRequest, "query or prompt is required", nil)
	}
	if req.Webhook != nil && strings.TrimSpace(req.Webhook.URL) != "" {
		if err := validateWebhookURL(req.Webhook.URL); err != nil {
			return writeError(c, http.StatusBadRequest, "webhook url is invalid", err.Error())
		}
	}
	present := fieldPresence(normalized)

	sourceTypes, err := parseV2SearchSources(req.Sources)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	scrapeOpts, err := buildV1ResearchScrapeOptions(req.ScrapeOptions)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if scrapeOpts != nil && req.ScrapeOptions != nil {
		scrapeOpts.ProxyURL = h.resolveProxyURL(c, req.ScrapeOptions.Proxy)
	}
	cfg, resolvedOptions, err := resolveV1ResearchConfig(req, present, sourceTypes, scrapeOpts)
	if err != nil {
		return writeError(c, http.StatusBadRequest, err.Error(), nil)
	}
	if err := h.ensureSearchSourcesAvailable(cfg.Sources); err != nil {
		return writeError(c, http.StatusServiceUnavailable, err.Error(), nil)
	}
	cfg.OrgID = orgID

	job := h.jobStore.New(map[string]string{
		"kind":        "research",
		"api_version": "v1",
		"query":       h.redactStoredString(c, req.Query),
		"preset":      cfg.Preset,
		"org_id":      orgID,
		"user_id":     userID,
		"tier":        tier,
		"zdr_mode":    fmt.Sprintf("%t", h.zdrMode(c)),
	})
	_, _ = h.jobStore.Update(job.ID, func(current *jobs.Job) {
		storedQuery := h.redactStoredString(c, req.Query)
		current.Result = map[string]any{
			"query":   storedQuery,
			"steps":   []researchStep{{Name: "search", Status: "queued"}, {Name: "expand", Status: "queued"}, {Name: "extract", Status: "queued"}, {Name: "synthesize", Status: "queued"}},
			"sources": []researchSource{},
			"report":  "",
			"data":    initialResearchData(cfg),
		}
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(job.ID, sse.EventJobCreated, map[string]any{
			"jobId":    job.ID,
			"resource": "research",
			"query":    req.Query,
		})
	}

	if err := h.enqueueAsyncJob(c.UserContext(), asyncjobs.KindResearch, job.ID, asyncjobs.ResearchPayload{
		OrgID:         orgID,
		BlendMode:     cfg.BlendMode,
		Query:         req.Query,
		Prompt:        req.Prompt,
		SystemPrompt:  req.SystemPrompt,
		Preset:        cfg.Preset,
		Limit:         cfg.Limit,
		MaxIterations: cfg.MaxIterations,
		Sources:       toAsyncSearchSources(cfg.Sources),
		ScrapeOpts:    cfg.ScrapeOpts,
		Webhook:       req.Webhook,
		TimeoutSec:    cfg.TimeoutSec,
	}, "v1"); err != nil {
		markJobDispatchFailure(h.jobStore, job.ID, err)
		return writeError(c, http.StatusBadGateway, "failed to queue research job", err.Error())
	}

	h.recordUserActivity(c, "research.created", "research", map[string]interface{}{
		"jobId":   job.ID,
		"summary": "research queued",
		"query":   h.redactStoredString(c, req.Query),
	})

	return c.JSON(h.newAsyncCreateEnvelope(c, "research", job.ID, job.CreatedAt, job.ExpiresAt, "queued", resolvedOptions))
}

func (h *Handler) v1ResearchStatus(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "research" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	return c.JSON(researchStatusEnvelope{
		resourceStatusEnvelope: resourceStatusEnvelope{
			Success:   job.Status != jobs.StatusFailed,
			ID:        job.ID,
			Resource:  "research",
			Status:    mapResearchJobStatus(job.Status),
			CreatedAt: job.CreatedAt,
			ExpiresAt: job.ExpiresAt,
			Error:     job.Error,
		},
		Query:   stringFromAny(job.Result["query"], job.Meta["query"]),
		Steps:   decodeResearchSteps(job.Result["steps"]),
		Sources: decodeResearchSources(job.Result["sources"]),
		Report:  stringFromAny(job.Result["report"], ""),
		Data:    mapFromAny(job.Result["data"]),
	})
}

func (h *Handler) v1CancelResearch(c *fiber.Ctx) error {
	jobID := strings.TrimSpace(c.Params("id"))
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}
	if h.jobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "job store is not initialized", nil)
	}

	job, ok := h.jobStore.Get(jobID)
	if !ok || job == nil || job.Meta["kind"] != "research" {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}
	if cancelValue, ok := h.activeResearchCancels.Load(jobID); ok {
		if cancelFn, ok := cancelValue.(context.CancelFunc); ok {
			cancelFn()
		}
	}
	h.publishAsyncCancel(c.UserContext(), asyncjobs.KindResearch, jobID)
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusCancelled
		current.Error = "cancelled by user"
		current.Result = mergeResearchResult(current.Result, map[string]any{
			"steps": []researchStep{
				{Name: "search", Status: "cancelled"},
				{Name: "extract", Status: "cancelled"},
				{Name: "synthesize", Status: "cancelled"},
			},
		})
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"status": "cancelled"})
	}
	h.recordUserActivity(c, "research.cancelled", "research", map[string]interface{}{
		"jobId":   jobID,
		"summary": "research cancelled",
	})
	return c.JSON(fiber.Map{"success": true, "id": jobID, "status": "cancelled"})
}

func (h *Handler) runV1ResearchJob(ctx context.Context, jobID string, cfg v1ResearchRunConfig) {
	defer h.activeResearchCancels.Delete(jobID)

	runCtx, cancel := context.WithTimeout(ctx, time.Duration(cfg.TimeoutSec)*time.Second)
	defer cancel()

	h.updateResearchJob(jobID, jobs.StatusRunning, 5, "", []researchStep{
		{Name: "search", Status: "running"},
		{Name: "expand", Status: "queued"},
		{Name: "extract", Status: "queued"},
		{Name: "synthesize", Status: "queued"},
	}, nil, "", initialResearchData(cfg))
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobStarted, map[string]any{"progress": 5})
	}

	// Concurrency limits from config (defaults: 3 queries, 5 scrapes).
	searchPara := 3
	scrapePara := 5
	if h.cfg != nil && h.cfg.ResearchConcurrency > 0 {
		searchPara = h.cfg.ResearchConcurrency
	}
	if h.cfg != nil && h.cfg.ResearchScrapePara > 0 {
		scrapePara = h.cfg.ResearchScrapePara
	}

	queryQueue := []string{cfg.Query}
	if strings.TrimSpace(cfg.Prompt) != "" && !strings.EqualFold(strings.TrimSpace(cfg.Prompt), strings.TrimSpace(cfg.Query)) {
		queryQueue = append(queryQueue, cfg.Prompt)
	}

	seenQueries := map[string]struct{}{}
	queryPlan := make([]string, 0, cfg.MaxIterations)
	var sourcesMu sync.Mutex
	sourceIndex := map[string]researchSource{}
	discoveredSources := make([]researchSource, 0)
	totalSearchCount := 0
	extractedCount := 0
	executedFollowUps := 0

	for len(queryQueue) > 0 && len(queryPlan) < cfg.MaxIterations {
		if err := runCtx.Err(); err != nil {
			h.finishResearchAsCancelled(jobID)
			return
		}

		// Batch-dequeue up to searchPara queries (or remaining capacity).
		batchSize := min(min(searchPara, len(queryQueue)), cfg.MaxIterations-len(queryPlan))
		batch := make([]string, 0, batchSize)
		for len(batch) < batchSize && len(queryQueue) > 0 {
			query := strings.TrimSpace(queryQueue[0])
			queryQueue = queryQueue[1:]
			normalizedQuery := strings.ToLower(query)
			if query == "" {
				continue
			}
			if _, seen := seenQueries[normalizedQuery]; seen {
				continue
			}
			seenQueries[normalizedQuery] = struct{}{}
			batch = append(batch, query)
		}
		if len(batch) == 0 {
			continue
		}

		// --- Parallel search: fan-out all queries in this batch ---
		type searchBatchResult struct {
			query   string
			results []V2SearchResult
		}
		batchResults := make([]searchBatchResult, len(batch))
		searchGroup, searchCtx := errgroup.WithContext(runCtx)
		searchGroup.SetLimit(searchPara)

		for i, query := range batch {
			i, query := i, query
			searchGroup.Go(func() error {
				results, err := h.executeV2Search(searchCtx, v2SearchRunConfig{
					OrgID:        cfg.OrgID,
					BlendMode:    cfg.BlendMode,
					Query:        query,
					Limit:        cfg.Limit,
					Sources:      cfg.Sources,
					ShouldScrape: false,
					TimeoutSec:   cfg.TimeoutSec,
				}, nil)
				if err != nil {
					return fmt.Errorf("search %q: %w", query, err)
				}
				batchResults[i] = searchBatchResult{query: query, results: results}
				return nil
			})
		}

		searchErr := searchGroup.Wait()
		if searchErr != nil {
			if runCtx.Err() != nil || errors.Is(searchErr, context.Canceled) {
				h.finishResearchAsCancelled(jobID)
				return
			}
			// If no queries have succeeded at all yet, fail the job.
			anySuccess := false
			for _, br := range batchResults {
				if br.results != nil {
					anySuccess = true
					break
				}
			}
			if !anySuccess && len(queryPlan) == 0 {
				h.failResearchJob(jobID, searchErr)
				return
			}
		}

		// Collect all new sources from the batch.
		var roundNewSources []researchSource
		for _, br := range batchResults {
			if br.results == nil {
				continue
			}
			queryPlan = append(queryPlan, br.query)
			if len(queryPlan) > 1 {
				executedFollowUps++
			}
			roundSources := toResearchSources(br.results)
			newSources := mergeResearchSources(sourceIndex, roundSources)
			roundNewSources = append(roundNewSources, newSources...)
		}
		totalSearchCount = len(sourceIndex)

		h.updateResearchJob(jobID, jobs.StatusRunning, researchProgressForIteration(len(queryPlan), cfg.MaxIterations, 30), "", []researchStep{
			{Name: "search", Status: "completed", Count: totalSearchCount},
			{Name: "expand", Status: researchExpandStatus(len(queryPlan), cfg.MaxIterations), Count: executedFollowUps},
			{Name: "extract", Status: "running", Count: extractedCount},
			{Name: "synthesize", Status: "queued"},
		}, orderedResearchSources(sourceIndex), "", map[string]any{
			"preset":        cfg.Preset,
			"blendMode":     cfg.BlendMode,
			"iterations":    len(queryPlan),
			"maxIterations": cfg.MaxIterations,
			"queryPlan":     append([]string(nil), queryPlan...),
		})

		// --- Parallel scrape: fan-out source fetching ---
		scrapeGroup, scrapeCtx := errgroup.WithContext(runCtx)
		scrapeGroup.SetLimit(scrapePara)

		for _, item := range roundNewSources {
			item := item
			if !isScrapableSearchURL(item.URL) {
				sourcesMu.Lock()
				extractedCount++
				mergeSourceContent(sourceIndex, item)
				discoveredSources = append(discoveredSources, item)
				sourcesMu.Unlock()
				continue
			}
			scrapeGroup.Go(func() error {
				outputs, _, fetchErr := h.fetchFormats(scrapeCtx, item.URL, cfg.ScrapeOpts)
				if fetchErr == nil {
					item.Content = firstSearchContent(outputs)
				} else if errors.Is(fetchErr, context.Canceled) {
					return fetchErr
				}
				sourcesMu.Lock()
				extractedCount++
				mergeSourceContent(sourceIndex, item)
				discoveredSources = append(discoveredSources, item)
				sourcesMu.Unlock()
				return nil
			})
		}

		if scrapeErr := scrapeGroup.Wait(); scrapeErr != nil {
			if errors.Is(scrapeErr, context.Canceled) {
				h.finishResearchAsCancelled(jobID)
				return
			}
		}

		h.updateResearchJob(jobID, jobs.StatusRunning, researchProgressForIteration(extractedCount, max(totalSearchCount, 1), 75), "", []researchStep{
			{Name: "search", Status: "completed", Count: totalSearchCount},
			{Name: "expand", Status: researchExpandStatus(len(queryPlan), cfg.MaxIterations), Count: executedFollowUps},
			{Name: "extract", Status: "running", Count: extractedCount},
			{Name: "synthesize", Status: "queued"},
		}, orderedResearchSources(sourceIndex), "", map[string]any{
			"preset":        cfg.Preset,
			"blendMode":     cfg.BlendMode,
			"iterations":    len(queryPlan),
			"maxIterations": cfg.MaxIterations,
			"queryPlan":     append([]string(nil), queryPlan...),
		})

		followUps := h.generateResearchFollowUps(runCtx, cfg, discoveredSources, seenQueries, cfg.MaxIterations-len(queryPlan))
		queryQueue = append(queryQueue, followUps...)
	}

	finalSources := orderedResearchSources(sourceIndex)
	report := buildResearchReport(cfg.Query, finalSources)
	data := map[string]any{
		"query":         cfg.Query,
		"preset":        cfg.Preset,
		"blendMode":     cfg.BlendMode,
		"sourceCount":   len(finalSources),
		"generatedAt":   time.Now().UTC().Format(time.RFC3339),
		"highlights":    researchHighlights(finalSources),
		"systemPrompt":  cfg.SystemPrompt,
		"iterations":    len(queryPlan),
		"maxIterations": cfg.MaxIterations,
		"queryPlan":     append([]string(nil), queryPlan...),
	}
	h.updateResearchJob(jobID, jobs.StatusReady, 100, "", []researchStep{
		{Name: "search", Status: "completed", Count: totalSearchCount},
		{Name: "expand", Status: "completed", Count: executedFollowUps},
		{Name: "extract", Status: "completed", Count: extractedCount},
		{Name: "synthesize", Status: "completed"},
	}, finalSources, report, data)
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobCompleted, map[string]any{"progress": 100})
	}
	if cfg.Webhook != nil && cfg.Webhook.URL != "" {
		go h.fireWebhook(cfg.Webhook.URL, &models.WebhookPayload{
			Success: true,
			Type:    "research.completed",
			ID:      jobID,
		})
	}
}

func (h *Handler) failResearchJob(jobID string, err error) {
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusFailed
		current.Error = err.Error()
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"error": err.Error()})
	}
}

func (h *Handler) finishResearchAsCancelled(jobID string) {
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = jobs.StatusCancelled
		if current.Error == "" {
			current.Error = "cancelled by user"
		}
	})
	if h.streamManager != nil {
		h.streamManager.Broadcast(jobID, sse.EventJobFailed, map[string]any{"status": "cancelled"})
	}
}

func (h *Handler) updateResearchJob(jobID string, status jobs.Status, progress int, errMsg string, steps []researchStep, sources []researchSource, report string, data map[string]any) {
	if h.jobStore == nil {
		return
	}
	if h.isJobZDR(jobID) {
		sources, report, data = sanitizeResearchPayloadForZDR(steps)
	}
	_, _ = h.jobStore.Update(jobID, func(current *jobs.Job) {
		current.Status = status
		current.Progress = progress
		current.Error = errMsg
		current.Result = mergeResearchResult(current.Result, map[string]any{
			"query":   stringFromAny(current.Result["query"], current.Meta["query"]),
			"steps":   steps,
			"sources": sources,
			"report":  report,
			"data":    data,
		})
	})
}

func mergeResearchSources(index map[string]researchSource, sources []researchSource) []researchSource {
	if index == nil {
		return nil
	}
	newSources := make([]researchSource, 0, len(sources))
	for _, item := range sources {
		urlKey := strings.TrimSpace(item.URL)
		if urlKey == "" {
			continue
		}
		if existing, ok := index[urlKey]; ok {
			if strings.TrimSpace(existing.Content) == "" && strings.TrimSpace(item.Content) != "" {
				existing.Content = item.Content
				index[urlKey] = existing
			}
			continue
		}
		index[urlKey] = item
		newSources = append(newSources, item)
	}
	return newSources
}

func mergeSourceContent(index map[string]researchSource, item researchSource) {
	if index == nil {
		return
	}
	urlKey := strings.TrimSpace(item.URL)
	if urlKey == "" {
		return
	}
	existing, ok := index[urlKey]
	if !ok {
		index[urlKey] = item
		return
	}
	if strings.TrimSpace(existing.Content) == "" && strings.TrimSpace(item.Content) != "" {
		existing.Content = item.Content
	}
	if strings.TrimSpace(existing.Snippet) == "" && strings.TrimSpace(item.Snippet) != "" {
		existing.Snippet = item.Snippet
	}
	index[urlKey] = existing
}

func orderedResearchSources(index map[string]researchSource) []researchSource {
	if len(index) == 0 {
		return nil
	}
	out := make([]researchSource, 0, len(index))
	for _, item := range index {
		out = append(out, item)
	}
	sort.SliceStable(out, func(i, j int) bool {
		return out[i].URL < out[j].URL
	})
	return out
}

func researchProgressForIteration(completed, total, ceiling int) int {
	if ceiling <= 0 {
		ceiling = 100
	}
	if total <= 0 {
		return 5
	}
	return min(ceiling, max(5, progressPercent(completed, total)*ceiling/100))
}

func researchExpandStatus(iterations, maxIterations int) string {
	if maxIterations <= 1 {
		return "completed"
	}
	if iterations >= maxIterations {
		return "completed"
	}
	if iterations > 0 {
		return "running"
	}
	return "queued"
}

func mergeResearchResult(current map[string]any, updates map[string]any) map[string]any {
	merged := map[string]any{}
	for key, value := range current {
		merged[key] = value
	}
	for key, value := range updates {
		if value == nil {
			continue
		}
		merged[key] = value
	}
	return merged
}

func mapResearchJobStatus(status jobs.Status) string {
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

func decodeResearchSteps(raw any) []researchStep {
	if raw == nil {
		return nil
	}
	encoded, err := json.Marshal(raw)
	if err != nil {
		return nil
	}
	var out []researchStep
	if err := json.Unmarshal(encoded, &out); err != nil {
		return nil
	}
	return out
}

func decodeResearchSources(raw any) []researchSource {
	if raw == nil {
		return nil
	}
	encoded, err := json.Marshal(raw)
	if err != nil {
		return nil
	}
	var out []researchSource
	if err := json.Unmarshal(encoded, &out); err != nil {
		return nil
	}
	return out
}

func mapFromAny(raw any) map[string]any {
	value, ok := raw.(map[string]any)
	if ok {
		return value
	}
	encoded, err := json.Marshal(raw)
	if err != nil {
		return nil
	}
	var out map[string]any
	if err := json.Unmarshal(encoded, &out); err != nil {
		return nil
	}
	return out
}

func toResearchSources(items []V2SearchResult) []researchSource {
	out := make([]researchSource, 0, len(items))
	for _, item := range items {
		out = append(out, researchSource{
			Title:   item.Title,
			URL:     item.URL,
			Snippet: item.Snippet,
			Source:  item.Source,
			Type:    item.Type,
			Content: item.Content,
		})
	}
	sort.SliceStable(out, func(i, j int) bool {
		return out[i].URL < out[j].URL
	})
	return out
}

func buildResearchReport(query string, sources []researchSource) string {
	var b strings.Builder
	b.WriteString("# Research Report\n\n")
	b.WriteString("Query: ")
	b.WriteString(strings.TrimSpace(query))
	b.WriteString("\n\n")
	for index, source := range sources {
		fmt.Fprintf(&b, "%d. %s\n", index+1, source.Title)
		if source.URL != "" {
			b.WriteString("   URL: ")
			b.WriteString(source.URL)
			b.WriteString("\n")
		}
		if strings.TrimSpace(source.Snippet) != "" {
			b.WriteString("   Snippet: ")
			b.WriteString(source.Snippet)
			b.WriteString("\n")
		}
		if strings.TrimSpace(source.Content) != "" {
			content := strings.TrimSpace(source.Content)
			if len(content) > 280 {
				content = content[:280] + "..."
			}
			b.WriteString("   Content: ")
			b.WriteString(strings.ReplaceAll(content, "\n", " "))
			b.WriteString("\n")
		}
		b.WriteString("\n")
	}
	return strings.TrimSpace(b.String())
}

func researchHighlights(sources []researchSource) []string {
	highlights := make([]string, 0, len(sources))
	for _, source := range sources {
		text := strings.TrimSpace(source.Content)
		if text == "" {
			text = strings.TrimSpace(source.Snippet)
		}
		if text == "" {
			continue
		}
		if len(text) > 160 {
			text = text[:160] + "..."
		}
		highlights = append(highlights, text)
		if len(highlights) == 5 {
			break
		}
	}
	return highlights
}

// generateResearchFollowUps tries to generate follow-up search queries using
// the AI backend (GenerateSearchQueries RPC). When the AI backend is
// unavailable or returns an unimplemented error, the function falls back to
// the heuristic implementation so the research job always makes progress.
func (h *Handler) generateResearchFollowUps(
	ctx context.Context,
	cfg v1ResearchRunConfig,
	sources []researchSource,
	seen map[string]struct{},
	remaining int,
) []string {
	if remaining <= 0 {
		return nil
	}

	seenSlice := make([]string, 0, len(seen))
	for q := range seen {
		seenSlice = append(seenSlice, q)
	}

	var aiQueryGen func(ctx context.Context, topic, summary string, seen []string, max int) ([]string, error)
	if h.scraper != nil && h.scraper.AIClient() != nil {
		cli := h.scraper.AIClient()
		aiQueryGen = func(ctx context.Context, topic, summary string, seen []string, max int) ([]string, error) {
			return cli.GenerateSearchQueries(ctx, topic, summary, seen, max)
		}
	}

	if aiQueryGen != nil {
		// Build a short findings summary from the top sources for context.
		var sb strings.Builder
		for i, s := range sources {
			if i >= 5 {
				break
			}
			if t := strings.TrimSpace(s.Title); t != "" {
				sb.WriteString(t)
				sb.WriteString(". ")
			}
			if sn := strings.TrimSpace(s.Snippet); sn != "" {
				if len(sn) > 120 {
					sn = sn[:120]
				}
				sb.WriteString(sn)
				sb.WriteString(" ")
			}
		}
		findingsSummary := strings.TrimSpace(sb.String())

		aiCtx, cancel := context.WithTimeout(ctx, 8*time.Second)
		defer cancel()
		queries, err := aiQueryGen(aiCtx, cfg.Query, findingsSummary, seenSlice, remaining)
		if err == nil && len(queries) > 0 {
			// Filter already-seen queries returned by the AI.
			out := make([]string, 0, len(queries))
			for _, q := range queries {
				norm := strings.ToLower(strings.TrimSpace(q))
				if norm == "" {
					continue
				}
				if _, alreadySeen := seen[norm]; alreadySeen {
					continue
				}
				out = append(out, q)
				if len(out) == remaining {
					break
				}
			}
			if len(out) > 0 {
				return out
			}
		}
	}

	// Fallback: heuristic follow-up generation.
	return buildResearchFollowUpQueries(cfg, sources, seen, remaining)
}
