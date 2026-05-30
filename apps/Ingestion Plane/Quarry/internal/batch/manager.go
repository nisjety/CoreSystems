package batch

import (
	"context"
	"fmt"
	neturl "net/url"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry/internal/models"
)

type Job struct {
	ID          string
	URLs        []string
	MaxAge      int64
	Webhook     *models.WebhookConfig
	Metadata    map[string]interface{}
	Status      string
	Total       int
	Completed   int
	Failed      int
	Results     []models.BatchScrapeResult
	Error       string
	CreatedAt   time.Time
	CompletedAt *time.Time
	ExpiresAt   time.Time
	mu          sync.RWMutex
}

type ScraperInterface interface {
	ScrapeCollection(ctx context.Context, req *models.ScrapeRequest) (*models.ScrapeResult, error)
}

type Manager struct {
	jobs           map[string]*Job
	mu             sync.RWMutex
	maxWorkers     int
	ttl            time.Duration
	scraper        ScraperInterface
	webhook        *WebhookDelivery
	cancels        map[string]context.CancelFunc
	done           chan struct{}
	closeOnce      sync.Once
	resultCallback func(jobID string, result models.BatchScrapeResult) // per-URL streaming callback
}

func NewManager(scraper ScraperInterface, maxWorkers int, ttl time.Duration, webhookSecret string) *Manager {
	if maxWorkers <= 0 {
		maxWorkers = 4
	}
	if ttl <= 0 {
		ttl = 24 * time.Hour
	}
	m := &Manager{
		jobs:       make(map[string]*Job),
		maxWorkers: maxWorkers,
		ttl:        ttl,
		scraper:    scraper,
		webhook:    NewWebhookDelivery(webhookSecret),
		cancels:    make(map[string]context.CancelFunc),
		done:       make(chan struct{}),
	}
	go m.cleanupExpiredJobs()
	return m
}

// SetResultCallback installs a per-URL result callback that fires every time
// a batch URL completes. Use this to wire SSE streaming.
func (m *Manager) SetResultCallback(cb func(jobID string, result models.BatchScrapeResult)) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.resultCallback = cb
}

func (m *Manager) CreateJob(req *models.BatchScrapeRequest) *Job {
	job := &Job{
		ID:        uuid.NewString(),
		URLs:      req.URLs,
		MaxAge:    req.MaxAge,
		Webhook:   req.Webhook,
		Metadata:  req.Metadata,
		Status:    "queued",
		Total:     len(req.URLs),
		Results:   make([]models.BatchScrapeResult, 0, len(req.URLs)),
		CreatedAt: time.Now(),
		ExpiresAt: time.Now().Add(m.ttl),
	}

	m.mu.Lock()
	m.jobs[job.ID] = job
	m.mu.Unlock()
	return job
}

func (m *Manager) GetJob(jobID string) *Job {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.jobs[jobID]
}

func (m *Manager) GetJobStatus(jobID string) *models.BatchScrapeStatus {
	job := m.GetJob(jobID)
	if job == nil {
		return nil
	}
	job.mu.RLock()
	defer job.mu.RUnlock()
	return &models.BatchScrapeStatus{
		Status:      job.Status,
		Total:       job.Total,
		Completed:   job.Completed,
		Failed:      job.Failed,
		ExpiresAt:   job.ExpiresAt,
		Data:        job.Results,
		Error:       job.Error,
		CreatedAt:   job.CreatedAt,
		CompletedAt: job.CompletedAt,
	}
}

func (m *Manager) StartJob(ctx context.Context, jobID string) error {
	job := m.GetJob(jobID)
	if job == nil {
		return ErrJobNotFound
	}
	job.mu.Lock()
	if job.Status != "queued" {
		job.mu.Unlock()
		return ErrJobAlreadyStarted
	}
	job.Status = "processing"
	job.mu.Unlock()

	m.dispatchWebhook(job, "batch_scrape.started", nil, "")

	runCtx, cancel := context.WithCancel(ctx)
	m.mu.Lock()
	m.cancels[job.ID] = cancel
	m.mu.Unlock()

	go m.processJob(runCtx, job)
	return nil
}

func (m *Manager) processJob(ctx context.Context, job *Job) {
	defer func() {
		m.mu.Lock()
		delete(m.cancels, job.ID)
		m.mu.Unlock()

		now := time.Now()
		job.mu.Lock()
		switch {
		case job.Status == "cancelled":
			job.Error = "cancelled by user"
		case job.Failed == job.Total:
			job.Status = "failed"
			job.Error = "all URLs failed"
		default:
			job.Status = "completed"
		}
		job.CompletedAt = &now
		status := &models.BatchScrapeStatus{
			Status:      job.Status,
			Total:       job.Total,
			Completed:   job.Completed,
			Failed:      job.Failed,
			ExpiresAt:   job.ExpiresAt,
			Data:        append([]models.BatchScrapeResult(nil), job.Results...),
			Error:       job.Error,
			CreatedAt:   job.CreatedAt,
			CompletedAt: job.CompletedAt,
		}
		job.mu.Unlock()

		if status.Status == "failed" {
			m.dispatchWebhook(job, "batch_scrape.failed", nil, status.Error)
		} else if status.Status == "completed" {
			m.dispatchWebhook(job, "batch_scrape.completed", status.Data, "")
		}
	}()

	sem := make(chan struct{}, m.maxWorkers)
	var wg sync.WaitGroup

	for _, targetURL := range job.URLs {
		wg.Add(1)
		sem <- struct{}{}
		go func(url string) {
			defer wg.Done()
			defer func() { <-sem }()

			result := models.BatchScrapeResult{URL: url}
			if err := ctx.Err(); err != nil {
				result.Success = false
				result.Error = err.Error()
				job.mu.Lock()
				job.Results = append(job.Results, result)
				job.Failed++
				job.mu.Unlock()
				return
			}
			collection := extractCollection(url)
			if collection == "" {
				result.Success = false
				result.Error = "could not infer collection"
			} else {
				scrapeResult, err := m.scraper.ScrapeCollection(ctx, &models.ScrapeRequest{
					Collection:  collection,
					MaxPages:    1,
					Enrich:      false,
					EnrichLimit: 0,
					MaxAge:      job.MaxAge,
				})
				if err != nil {
					result.Success = false
					result.Error = err.Error()
				} else {
					result.Success = true
					result.Count = scrapeResult.Count
					if scrapeResult.Products != nil {
						result.Products = make([]models.Product, 0, len(scrapeResult.Products))
						for _, p := range scrapeResult.Products {
							result.Products = append(result.Products, *p)
						}
					}
				}
			}

			job.mu.Lock()
			job.Results = append(job.Results, result)
			if result.Success {
				job.Completed++
			} else {
				job.Failed++
			}
			job.mu.Unlock()

			if ctx.Err() == nil {
				m.dispatchWebhook(job, "batch_scrape.page", []models.BatchScrapeResult{result}, result.Error)
				// Stream per-URL result to SSE subscribers.
				m.mu.RLock()
				cb := m.resultCallback
				m.mu.RUnlock()
				if cb != nil {
					cb(job.ID, result)
				}
			}
		}(targetURL)
	}

	wg.Wait()
	log.Info().Str("job_id", job.ID).Int("completed", job.Completed).Int("failed", job.Failed).Msg("batch job finished")
}

func (m *Manager) WaitForCompletion(ctx context.Context, jobID string, timeout time.Duration, pollInterval time.Duration) (*models.BatchScrapeStatus, error) {
	if pollInterval <= 0 {
		pollInterval = 500 * time.Millisecond
	}
	deadline := time.Now().Add(timeout)
	ticker := time.NewTicker(pollInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-ticker.C:
			status := m.GetJobStatus(jobID)
			if status == nil {
				return nil, ErrJobNotFound
			}
			if status.Status == "completed" || status.Status == "failed" || status.Status == "cancelled" {
				if status.Status == "cancelled" {
					return status, ErrJobCancelled
				}
				return status, nil
			}
			if timeout > 0 && time.Now().After(deadline) {
				return status, ErrJobTimeout
			}
		}
	}
}

func (m *Manager) CancelJob(jobID string) error {
	job := m.GetJob(jobID)
	if job == nil {
		return ErrJobNotFound
	}

	job.mu.Lock()
	if job.Status == "completed" || job.Status == "failed" || job.Status == "cancelled" {
		job.mu.Unlock()
		return nil
	}
	job.Status = "cancelled"
	job.Error = "cancelled by user"
	job.mu.Unlock()

	m.mu.Lock()
	cancel := m.cancels[jobID]
	delete(m.cancels, jobID)
	m.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	return nil
}

func (m *Manager) cleanupExpiredJobs() {
	ticker := time.NewTicker(time.Hour)
	defer ticker.Stop()
	for {
		select {
		case <-ticker.C:
			now := time.Now()
			m.mu.Lock()
			for id, job := range m.jobs {
				if now.After(job.ExpiresAt) {
					delete(m.jobs, id)
				}
			}
			m.mu.Unlock()
		case <-m.done:
			return
		}
	}
}

func extractCollection(targetURL string) string {
	if targetURL == "" {
		return ""
	}
	parsed, err := neturl.Parse(strings.TrimSpace(targetURL))
	if err != nil {
		return ""
	}
	segments := strings.Split(strings.Trim(strings.ToLower(parsed.Path), "/"), "/")
	for index, segment := range segments {
		if segment == "collections" || segment == "produktkategori" {
			if index+1 < len(segments) {
				return strings.TrimSpace(segments[index+1])
			}
		}
	}
	if len(segments) > 0 {
		return strings.TrimSpace(segments[len(segments)-1])
	}
	return ""
}

func (m *Manager) dispatchWebhook(job *Job, eventType string, data []models.BatchScrapeResult, errMsg string) {
	if m == nil || job == nil || job.Webhook == nil || strings.TrimSpace(job.Webhook.URL) == "" || m.webhook == nil {
		return
	}
	if !shouldSendWebhookEvent(job.Webhook.Events, eventType) {
		return
	}

	status := m.GetJobStatus(job.ID)
	payload := &models.WebhookPayload{
		Success:  !strings.Contains(eventType, "failed"),
		Type:     eventType,
		ID:       job.ID,
		Data:     data,
		Metadata: job.Metadata,
		Error:    errMsg,
		Status:   status,
	}

	go func(url string, body *models.WebhookPayload) {
		if sendErr := m.webhook.Send(url, body); sendErr != nil {
			log.Warn().Err(sendErr).Str("job_id", body.ID).Str("event", body.Type).Msg("webhook delivery failed")
		}
	}(job.Webhook.URL, payload)
}

func shouldSendWebhookEvent(events []string, eventType string) bool {
	if len(events) == 0 {
		return true
	}

	eventType = strings.ToLower(strings.TrimSpace(eventType))
	for _, event := range events {
		normalized := strings.ToLower(strings.TrimSpace(event))
		if normalized == "" {
			continue
		}
		if normalized == eventType {
			return true
		}
		switch normalized {
		case "started":
			if strings.HasSuffix(eventType, ".started") {
				return true
			}
		case "page":
			if strings.HasSuffix(eventType, ".page") {
				return true
			}
		case "completed":
			if strings.HasSuffix(eventType, ".completed") {
				return true
			}
		case "failed":
			if strings.HasSuffix(eventType, ".failed") {
				return true
			}
		}
	}

	return false
}

// Close stops the manager's background cleanup loop.
func (m *Manager) Close() error {
	if m == nil {
		return nil
	}
	m.closeOnce.Do(func() {
		close(m.done)
	})
	return nil
}

// DeliverWebhook sends a webhook payload synchronously (called from API handler goroutine).
func (m *Manager) DeliverWebhook(ctx context.Context, url string, payload *models.WebhookPayload) error {
	if m == nil || m.webhook == nil {
		return fmt.Errorf("webhook delivery not configured")
	}
	return m.webhook.Send(url, payload)
}
