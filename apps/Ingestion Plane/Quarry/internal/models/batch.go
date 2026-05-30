package models

import "time"

// BatchScrapeRequest represents a batch scrape request
type BatchScrapeRequest struct {
	URLs         []string               `json:"urls" validate:"required,min=1"`
	MaxAge       int64                  `json:"maxAge,omitempty"`       // Cache age in milliseconds
	WaitTimeout  int                    `json:"waitTimeout,omitempty"`  // Timeout in seconds for wait mode
	PollInterval int                    `json:"pollInterval,omitempty"` // Poll interval in seconds for wait mode
	Webhook      *WebhookConfig         `json:"webhook,omitempty"`
	Metadata     map[string]interface{} `json:"metadata,omitempty"`
}

// WebhookConfig configures webhook notifications
type WebhookConfig struct {
	URL      string                 `json:"url" validate:"required,url"`
	Events   []string               `json:"events,omitempty"` // started, page, completed, failed
	Metadata map[string]interface{} `json:"metadata,omitempty"`
}

// BatchScrapeStartResponse is returned when starting a batch job
type BatchScrapeStartResponse struct {
	Success bool   `json:"success"`
	ID      string `json:"id"`
	URL     string `json:"url"`
}

// BatchScrapeStatus represents the status of a batch scrape job
type BatchScrapeStatus struct {
	Status      string              `json:"status"` // queued, processing, completed, failed
	Total       int                 `json:"total"`
	Completed   int                 `json:"completed"`
	Failed      int                 `json:"failed"`
	ExpiresAt   time.Time           `json:"expiresAt"`
	Data        []BatchScrapeResult `json:"data,omitempty"`
	Error       string              `json:"error,omitempty"`
	CreatedAt   time.Time           `json:"createdAt"`
	CompletedAt *time.Time          `json:"completedAt,omitempty"`
}

// BatchScrapeResult represents the result of scraping a single URL
type BatchScrapeResult struct {
	URL      string                 `json:"url"`
	Success  bool                   `json:"success"`
	Products []Product              `json:"products,omitempty"`
	Count    int                    `json:"count"`
	Error    string                 `json:"error,omitempty"`
	Metadata map[string]interface{} `json:"metadata,omitempty"`
}

// WebhookPayload is sent to webhook URLs
type WebhookPayload struct {
	Success  bool                   `json:"success"`
	Type     string                 `json:"type"` // batch_scrape.started, batch_scrape.page, batch_scrape.completed, batch_scrape.failed
	ID       string                 `json:"id"`
	Data     []BatchScrapeResult    `json:"data,omitempty"`
	Metadata map[string]interface{} `json:"metadata,omitempty"`
	Error    string                 `json:"error,omitempty"`
	Status   *BatchScrapeStatus     `json:"status,omitempty"`
}
