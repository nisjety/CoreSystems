package api

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/google/uuid"
	zlog "github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/dataplane"
	"github.com/triodelab/quarry/internal/jobs"
	"github.com/triodelab/quarry/internal/scraper"
)

// ExtractRequest represents an async extraction job request.
// Schema accepts either a JSON object/array or a JSON string containing schema JSON.
type ExtractRequest struct {
	URL     string          `json:"url"`
	Schema  json.RawMessage `json:"schema,omitempty"`
	Prompt  string          `json:"prompt,omitempty"`
	Timeout int             `json:"timeout,omitempty"` // Timeout in seconds (default 30)
	MaxAge  int64           `json:"maxAge,omitempty"`  // Optional cache age in milliseconds
}

// ExtractResponse represents the response from queuing an extraction job
type ExtractResponse struct {
	Success           bool   `json:"success"`
	JobID             string `json:"jobId"`
	Status            string `json:"status"`
	EstimatedWaitTime int    `json:"estimatedWaitTime"` // seconds
}

// ExtractStatusResponse represents the status response for an extraction job
type ExtractStatusResponse struct {
	Success    bool                   `json:"success"`
	JobID      string                 `json:"jobId"`
	Status     string                 `json:"status"`
	Result     map[string]interface{} `json:"result,omitempty"`
	Error      string                 `json:"error,omitempty"`
	DurationMs int64                  `json:"duration_ms"`
	ExpiresAt  string                 `json:"expires_at"`
}

// handleExtract queues an extraction job and returns immediately with jobId
func (h *Handler) handleExtract(c *fiber.Ctx) error {
	var req ExtractRequest
	if err := c.BodyParser(&req); err != nil {
		return writeError(c, http.StatusBadRequest, "invalid request body", nil)
	}

	req.URL = strings.TrimSpace(req.URL)
	req.Prompt = strings.TrimSpace(req.Prompt)

	if req.URL == "" {
		return writeError(c, http.StatusBadRequest, "url is required", nil)
	}
	if err := validateAbsoluteHTTPURL(req.URL); err != nil {
		return writeError(c, http.StatusBadRequest, fmt.Sprintf("invalid url: %v", err), nil)
	}
	if len(bytes.TrimSpace(req.Schema)) == 0 && req.Prompt == "" {
		return writeError(c, http.StatusBadRequest, "schema or prompt is required", nil)
	}

	normalizedSchema, err := h.normalizeSchema(req.Schema)
	if err != nil {
		return writeError(c, http.StatusBadRequest, "schema must be valid JSON or a JSON-encoded string", err.Error())
	}

	jobID := fmt.Sprintf("extract_%s", uuid.New().String()[:8])
	now := time.Now()
	job := &jobs.ExtractionJob{
		ID:        jobID,
		URL:       req.URL,
		Schema:    normalizedSchema,
		Prompt:    req.Prompt,
		Status:    jobs.ExtractionQueued,
		CreatedAt: now,
		UpdatedAt: now,
	}

	ctx, cancel := context.WithTimeout(c.UserContext(), 5*time.Second)
	defer cancel()

	if err := h.extractionJobStore.Create(ctx, job); err != nil {
		zlog.Error().Err(err).Str("job_id", jobID).Msg("failed to create extraction job")
		return writeError(c, http.StatusInternalServerError, "failed to queue extraction job", nil)
	}

	// The background worker owns the request after the 202 response; it uses its
	// own timeout so completion is independent of client disconnects.
	go h.processExtractionJob(context.Background(), jobID, req.Timeout, req.MaxAge)

	zlog.Info().
		Str("jobId", jobID).
		Str("url", req.URL).
		Bool("hasSchema", normalizedSchema != "").
		Bool("hasPrompt", req.Prompt != "").
		Msg("extraction job queued")

	return c.Status(http.StatusAccepted).JSON(ExtractResponse{
		Success:           true,
		JobID:             jobID,
		Status:            string(jobs.ExtractionQueued),
		EstimatedWaitTime: 5, // seconds (rough estimate)
	})
}

// handleExtractStatus returns the status and result of an extraction job
func (h *Handler) handleExtractStatus(c *fiber.Ctx) error {
	if h.extractionJobStore == nil {
		return writeError(c, http.StatusServiceUnavailable, "extraction job store is not initialized", nil)
	}

	jobID := c.Params("id")
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}

	ctx, cancel := context.WithTimeout(c.UserContext(), 5*time.Second)
	defer cancel()

	job, err := h.extractionJobStore.Get(ctx, jobID)
	if err != nil {
		if err == jobs.ErrJobNotFound {
			return writeError(c, http.StatusNotFound, "job not found", nil)
		}
		zlog.Error().Err(err).Str("job_id", jobID).Msg("failed to get job")
		return writeError(c, http.StatusInternalServerError, "failed to retrieve job status", nil)
	}

	return c.Status(http.StatusOK).JSON(ExtractStatusResponse{
		Success:    true,
		JobID:      jobID,
		Status:     string(job.Status),
		Result:     job.Result,
		Error:      job.Error,
		DurationMs: job.Duration,
		ExpiresAt:  job.ExpiresAt.Format(time.RFC3339),
	})
}

// processExtractionJob processes an extraction job asynchronously.
func (h *Handler) processExtractionJob(ctx context.Context, jobID string, timeoutSec int, maxAge int64) {
	defer func() {
		if r := recover(); r != nil {
			zlog.Error().Interface("panic", r).Str("job_id", jobID).Msg("extraction job panic")
		}
	}()

	// Set timeout for extraction
	if timeoutSec <= 0 || timeoutSec > 300 {
		timeoutSec = 30
	}
	runCtx, cancel := context.WithTimeout(ctx, time.Duration(timeoutSec)*time.Second)
	defer cancel()

	job, err := h.extractionJobStore.Get(runCtx, jobID)
	if err != nil {
		zlog.Error().Err(err).Str("job_id", jobID).Msg("failed to get job for processing")
		return
	}

	// Update job status to processing
	job.Status = jobs.ExtractionProcessing
	if err := h.extractionJobStore.Update(runCtx, job); err != nil {
		zlog.Error().Err(err).Str("job_id", jobID).Msg("failed to update job status to processing")
		return
	}

	result, scrapeErr := h.performExtraction(runCtx, job, maxAge)

	if scrapeErr != nil {
		job.Status = jobs.ExtractionFailed
		job.Error = scrapeErr.Error()
	} else {
		job.Status = jobs.ExtractionCompleted
		job.Result = result
		job.Error = ""
	}

	if err := h.extractionJobStore.Update(runCtx, job); err != nil {
		zlog.Error().Err(err).Str("job_id", jobID).Msg("failed to update job with result")
		return
	}

	zlog.Info().Str("job_id", jobID).Str("status", string(job.Status)).Msg("extraction job complete")
}

// performExtraction executes the single-page extraction flow against the shared
// scraper so fetch, cache, and AI extraction behavior stay consistent.
func (h *Handler) performExtraction(ctx context.Context, job *jobs.ExtractionJob, maxAge int64) (map[string]interface{}, error) {
	return h.extractStructured(ctx, job.URL, &scraper.StructuredExtractOptions{
		Schema:   job.Schema,
		Prompt:   job.Prompt,
		MaxAgeMs: maxAge,
	})
}

// IngestRequest represents the request body for ingesting extracted content
type IngestRequest struct {
	OrgID string `json:"org_id,omitempty"`
}

// IngestResponse represents the response from ingesting extracted content to the data plane
type IngestResponse struct {
	Success    bool   `json:"success"`
	JobID      string `json:"jobId"`
	DocumentID string `json:"documentId,omitempty"`
	Status     string `json:"status"`
	Error      string `json:"error,omitempty"`
}

// handleExtractIngest ingests a completed extraction job into the data plane
func (h *Handler) handleExtractIngest(c *fiber.Ctx) error {
	jobID := c.Params("id")
	if jobID == "" {
		return writeError(c, http.StatusBadRequest, "job id is required", nil)
	}

	var req IngestRequest
	_ = c.BodyParser(&req) // Optional body

	// Try query param if not in body
	if req.OrgID == "" {
		req.OrgID = c.Query("org_id")
	}

	if req.OrgID == "" {
		return writeError(c, http.StatusBadRequest, "org_id is required (body or query param)", nil)
	}

	// Get job from store
	job, err := h.extractionJobStore.Get(c.UserContext(), jobID)
	if err != nil {
		return writeError(c, http.StatusInternalServerError, "failed to retrieve job", map[string]interface{}{"error": err.Error()})
	}

	if job == nil {
		return writeError(c, http.StatusNotFound, "job not found", nil)
	}

	// Check job status
	if job.Status != jobs.ExtractionCompleted {
		return writeError(c, http.StatusBadRequest, fmt.Sprintf("job status is %s, not completed", job.Status), nil)
	}

	// Use result from job
	result := job.Result
	if result == nil {
		result = make(map[string]interface{})
	}

	// Extract title from URL
	title := job.URL
	if len(title) > 50 {
		title = title[:50] + "..."
	}

	// Build document request for data plane
	docReq := &dataplane.DocumentCreateRequest{
		OrgID:   req.OrgID,
		Source:  "quarry",
		Type:    "extraction",
		Title:   title,
		Content: fmt.Sprintf("%v", result),
		Metadata: map[string]interface{}{
			"job_id":     jobID,
			"url":        job.URL,
			"schema":     job.Schema,
			"updated_at": job.UpdatedAt,
		},
	}

	// Ingest into data plane
	ctx, cancel := context.WithTimeout(c.UserContext(), 30*time.Second)
	defer cancel()

	docResp, err := h.ingestExtraction(ctx, req.OrgID, docReq)
	if err != nil {
		zlog.Error().Err(err).Str("job_id", jobID).Msg("failed to ingest extraction into data plane")
		return writeError(c, http.StatusInternalServerError, "failed to ingest into data plane", map[string]interface{}{"error": err.Error()})
	}

	return c.Status(http.StatusOK).JSON(IngestResponse{
		Success:    true,
		JobID:      jobID,
		DocumentID: docResp.DocumentID,
		Status:     "ingested",
	})
}

func (h *Handler) normalizeSchema(raw json.RawMessage) (string, error) {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 {
		return "", nil
	}

	cacheKey := string(trimmed)
	if h.schemaCache != nil {
		if cached, ok := h.schemaCache.Get(cacheKey); ok {
			return cached, nil
		}
	}

	if trimmed[0] == '"' {
		var schemaString string
		if err := json.Unmarshal(trimmed, &schemaString); err != nil {
			return "", err
		}
		compacted, err := CompactJSON(schemaString)
		if err != nil {
			return "", err
		}
		if h.schemaCache != nil {
			h.schemaCache.Set(cacheKey, compacted)
		}
		return compacted, nil
	}

	compacted, err := CompactJSON(string(trimmed))
	if err != nil {
		return "", err
	}
	if h.schemaCache != nil {
		h.schemaCache.Set(cacheKey, compacted)
	}
	return compacted, nil
}
