package models

import (
	"time"

	"github.com/triodelab/quarry/internal/driver"
)

type ScrapeAPIRequest struct {
	URL         string                 `json:"url"`
	Module      string                 `json:"module,omitempty"`
	Collection  string                 `json:"collection,omitempty"`
	MaxPages    int                    `json:"maxPages,omitempty"`
	Enrich      bool                   `json:"enrich,omitempty"`
	EnrichLimit int                    `json:"enrichLimit,omitempty"`
	MaxAge      int64                  `json:"maxAge,omitempty"`
	ChangeTrack *ChangeTrackingRequest `json:"changeTracking,omitempty"`
	Formats     []string               `json:"formats,omitempty"`
	IncludeTags []string               `json:"includeTags,omitempty"`
	ExcludeTags []string               `json:"excludeTags,omitempty"`
	OnlyMain    bool                   `json:"onlyMainContent,omitempty"`
	WaitFor     int                    `json:"waitFor,omitempty"`
	Headers     map[string]string      `json:"headers,omitempty"`
	Actions     []ActionRequest        `json:"actions,omitempty"`
	ParserMode  string                 `json:"parserMode,omitempty"`

	// Browser emulation
	Mobile   bool                   `json:"mobile,omitempty"`
	Viewport *driver.ViewportConfig `json:"viewport,omitempty"`
	Location *driver.GeoLocation    `json:"location,omitempty"`
	BlockAds bool                   `json:"blockAds,omitempty"`
	Proxy    *ProxyConfig           `json:"proxy,omitempty"`
}

type ProxyConfig struct {
	URL    string `json:"url,omitempty"`
	Type   string `json:"type,omitempty"`
	Region string `json:"region,omitempty"`
}

type ActionRequest struct {
	Type         string `json:"type"`
	Selector     string `json:"selector,omitempty"`
	Text         string `json:"text,omitempty"`
	Key          string `json:"key,omitempty"`
	Script       string `json:"script,omitempty"`       // for executeJavascript
	Milliseconds int    `json:"milliseconds,omitempty"` // for wait
	Direction    string `json:"direction,omitempty"`
	FullPage     bool   `json:"fullPage,omitempty"`
	Retry        int    `json:"retry,omitempty"`
	AfterShot    bool   `json:"screenshotAfter,omitempty"`
}

type CrawlAPIRequest struct {
	URL         string            `json:"url"`
	Module      string            `json:"module,omitempty"`
	Mode        string            `json:"mode,omitempty"`
	MaxDepth    int               `json:"maxDepth,omitempty"`
	MaxPages    int               `json:"maxPages,omitempty"`
	ScheduleAt  *time.Time        `json:"scheduleAt,omitempty"`
	Enrich      bool              `json:"enrich,omitempty"`
	EnrichLimit int               `json:"enrichLimit,omitempty"`
	MaxAge      int64             `json:"maxAge,omitempty"`
	Headers     map[string]string `json:"headers,omitempty"`
	// Webhook fires after the crawl job completes or fails.
	Webhook *WebhookConfig `json:"webhook,omitempty"`
	// Prompt guides smart-crawl extraction and URL filtering.
	Prompt string `json:"prompt,omitempty"`
	// Schema is a user-defined JSON schema for custom extraction.
	Schema string `json:"schema,omitempty"`
	// IncludePaths are glob/regex patterns — only URLs matching at least one are crawled.
	IncludePaths []string `json:"includePaths,omitempty"`
	// ExcludePaths are glob/regex patterns — URLs matching any are skipped.
	ExcludePaths []string `json:"excludePaths,omitempty"`
	// EventSink receives real-time page-level events (not serialised).
	EventSink PageEventSink `json:"-"`

	// Browser emulation for crawled pages
	Mobile   bool                   `json:"mobile,omitempty"`
	Viewport *driver.ViewportConfig `json:"viewport,omitempty"`
	Location *driver.GeoLocation    `json:"location,omitempty"`
	BlockAds bool                   `json:"blockAds,omitempty"`

	// Output format selection (default: markdown)
	Formats []string `json:"formats,omitempty"`
}
