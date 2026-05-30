package asyncjobs

import (
	"encoding/json"
	"fmt"
	"strings"
	"time"

	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
	"github.com/triodelab/quarry/internal/models"
	"github.com/triodelab/quarry/internal/scraper"
)

const (
	StreamName    = "VELION_INGESTION"
	CancelSubject = "velion.ingestion.jobs.cancel"
)

type Kind string

const (
	KindCrawl    Kind = "crawl"
	KindSearch   Kind = "search"
	KindExtract  Kind = "extract"
	KindResearch Kind = "research"
	KindAgent    Kind = "agent"
	KindLlmsTxt  Kind = "llmstxt"
)

var AllKinds = []Kind{
	KindCrawl,
	KindSearch,
	KindExtract,
	KindResearch,
	KindAgent,
	KindLlmsTxt,
}

type Message struct {
	Kind       Kind            `json:"kind"`
	JobID      string          `json:"jobId"`
	QueuedAt   time.Time       `json:"queuedAt"`
	Payload    json.RawMessage `json:"payload"`
	APIVersion string          `json:"apiVersion,omitempty"`
}

type CancelMessage struct {
	Kind        Kind      `json:"kind"`
	JobID       string    `json:"jobId"`
	RequestedAt time.Time `json:"requestedAt"`
}

type CrawlPayload struct {
	Spec    quarrycrawl.Spec      `json:"spec"`
	Webhook *models.WebhookConfig `json:"webhook,omitempty"`
	OrgID   string                `json:"orgId,omitempty"`
	UserID  string                `json:"userId,omitempty"`
}

type SearchSource struct {
	Type          string  `json:"type"`
	Site          string  `json:"site,omitempty"`
	Weight        float64 `json:"weight,omitempty"`
	Limit         int     `json:"limit,omitempty"`
	Country       string  `json:"country,omitempty"`
	SearchLang    string  `json:"searchLang,omitempty"`
	UILang        string  `json:"uiLang,omitempty"`
	Freshness     string  `json:"freshness,omitempty"`
	SafeSearch    string  `json:"safeSearch,omitempty"`
	ExtraSnippets bool    `json:"extraSnippets,omitempty"`
}

type SearchPayload struct {
	Preset       string                 `json:"preset,omitempty"`
	OrgID        string                 `json:"orgId,omitempty"`
	BlendMode    string                 `json:"blendMode,omitempty"`
	Query        string                 `json:"query"`
	Limit        int                    `json:"limit"`
	Sources      []SearchSource         `json:"sources,omitempty"`
	ScrapeOpts   *scraper.FormatOptions `json:"scrapeOptions,omitempty"`
	ShouldScrape bool                   `json:"shouldScrape,omitempty"`
	TimeoutSec   int                    `json:"timeout"`
	Webhook      *models.WebhookConfig  `json:"webhook,omitempty"`
}

type ExtractPayload struct {
	Preset       string                 `json:"preset,omitempty"`
	OrgID        string                 `json:"orgId,omitempty"`
	UserID       string                 `json:"userId,omitempty"`
	Schema       string                 `json:"schema,omitempty"`
	Prompt       string                 `json:"prompt,omitempty"`
	SystemPrompt string                 `json:"systemPrompt,omitempty"`
	TimeoutSec   int                    `json:"timeout"`
	URLTrace     []string               `json:"urlTrace"`
	Webhook      *models.WebhookConfig  `json:"webhook,omitempty"`
	ScrapeFormat *scraper.FormatOptions `json:"scrapeFormat,omitempty"`
}

type ResearchPayload struct {
	OrgID         string                 `json:"orgId,omitempty"`
	BlendMode     string                 `json:"blendMode,omitempty"`
	Query         string                 `json:"query"`
	Prompt        string                 `json:"prompt,omitempty"`
	SystemPrompt  string                 `json:"systemPrompt,omitempty"`
	Preset        string                 `json:"preset,omitempty"`
	Limit         int                    `json:"limit"`
	MaxIterations int                    `json:"maxIterations,omitempty"`
	Sources       []SearchSource         `json:"sources,omitempty"`
	ScrapeOpts    *scraper.FormatOptions `json:"scrapeOptions,omitempty"`
	Webhook       *models.WebhookConfig  `json:"webhook,omitempty"`
	TimeoutSec    int                    `json:"timeout"`
}

type AgentPayload struct {
	OrgID                 string                        `json:"orgId,omitempty"`
	UserID                string                        `json:"userId,omitempty"`
	Tier                  string                        `json:"tier,omitempty"`
	Objective             string                        `json:"objective"`
	URLs                  []string                      `json:"urls,omitempty"`
	Schema                string                        `json:"schema,omitempty"`
	Model                 string                        `json:"model,omitempty"`
	MaxSteps              int                           `json:"maxSteps,omitempty"`
	MaxCredits            int                           `json:"maxCredits,omitempty"`
	StrictConstrainToURLs bool                          `json:"strictConstrainToUrls,omitempty"`
	EnableWebSearch       bool                          `json:"enableWebSearch,omitempty"`
	AllowExternalLinks    bool                          `json:"allowExternalLinks,omitempty"`
	Module                string                        `json:"module,omitempty"`
	Collection            string                        `json:"collection,omitempty"`
	Context               map[string]interface{}        `json:"context,omitempty"`
	ChangeTrack           *models.ChangeTrackingRequest `json:"changeTracking,omitempty"`
	Webhook               *models.WebhookConfig         `json:"webhook,omitempty"`
	TimeoutSec            int                           `json:"timeout"`
}

type LlmsTxtPayload struct {
	OrgID      string                `json:"orgId,omitempty"`
	UserID     string                `json:"userId,omitempty"`
	URL        string                `json:"url"`
	Full       bool                  `json:"full,omitempty"`
	Webhook    *models.WebhookConfig `json:"webhook,omitempty"`
	TimeoutSec int                   `json:"timeout"`
}

func NewMessage(kind Kind, jobID string, payload any, apiVersion string) (Message, error) {
	trimmedJobID := strings.TrimSpace(jobID)
	if trimmedJobID == "" {
		return Message{}, fmt.Errorf("job id is required")
	}
	if strings.TrimSpace(string(kind)) == "" {
		return Message{}, fmt.Errorf("job kind is required")
	}

	data, err := json.Marshal(payload)
	if err != nil {
		return Message{}, fmt.Errorf("marshal payload: %w", err)
	}

	return Message{
		Kind:       kind,
		JobID:      trimmedJobID,
		QueuedAt:   time.Now().UTC(),
		Payload:    data,
		APIVersion: strings.TrimSpace(apiVersion),
	}, nil
}

func (m Message) DecodePayload(target any) error {
	if len(m.Payload) == 0 {
		return fmt.Errorf("payload is empty")
	}
	if target == nil {
		return fmt.Errorf("decode target is nil")
	}
	return json.Unmarshal(m.Payload, target)
}

func ExecuteSubject(kind Kind) string {
	return "velion.ingestion.jobs.execute." + strings.TrimSpace(string(kind))
}
