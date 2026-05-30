package client

import (
	"encoding/json"
	"time"
)

type AsyncCreateResponse struct {
	Success         bool                   `json:"success"`
	ID              string                 `json:"id"`
	Resource        string                 `json:"resource"`
	Status          string                 `json:"status"`
	CreatedAt       time.Time              `json:"createdAt"`
	ExpiresAt       time.Time              `json:"expiresAt"`
	StatusURL       string                 `json:"statusUrl"`
	EventsURL       string                 `json:"eventsUrl"`
	WebsocketURL    string                 `json:"websocketUrl,omitempty"`
	ResolvedOptions map[string]interface{} `json:"resolvedOptions,omitempty"`
}

type ProxyRequest struct {
	URL    string `json:"url,omitempty"`
	Type   string `json:"type,omitempty"`
	Region string `json:"region,omitempty"`
}

type Viewport struct {
	Width  int `json:"width,omitempty"`
	Height int `json:"height,omitempty"`
}

type GeoLocation struct {
	Latitude  float64 `json:"latitude,omitempty"`
	Longitude float64 `json:"longitude,omitempty"`
}

type ActionStep struct {
	Type     string                 `json:"type"`
	Selector string                 `json:"selector,omitempty"`
	Value    string                 `json:"value,omitempty"`
	Options  map[string]interface{} `json:"options,omitempty"`
}

type ScrapeOptions struct {
	Formats         []string          `json:"formats,omitempty"`
	Headers         map[string]string `json:"headers,omitempty"`
	WaitFor         int               `json:"waitFor,omitempty"`
	OnlyMainContent bool              `json:"onlyMainContent,omitempty"`
	IncludeTags     []string          `json:"includeTags,omitempty"`
	ExcludeTags     []string          `json:"excludeTags,omitempty"`
	Actions         []ActionStep      `json:"actions,omitempty"`
	Mobile          bool              `json:"mobile,omitempty"`
	Viewport        *Viewport         `json:"viewport,omitempty"`
	Location        *GeoLocation      `json:"location,omitempty"`
	BlockAds        bool              `json:"blockAds,omitempty"`
	MaxAge          int64             `json:"maxAge,omitempty"`
	ParserMode      string            `json:"parserMode,omitempty"`
	Proxy           *ProxyRequest     `json:"proxy,omitempty"`
}

type WebhookConfig struct {
	URL      string                 `json:"url"`
	Events   []string               `json:"events,omitempty"`
	Metadata map[string]interface{} `json:"metadata,omitempty"`
}

type ChangeTrackingRequest struct {
	Enabled bool     `json:"enabled,omitempty"`
	Modes   []string `json:"modes,omitempty"`
	Tag     string   `json:"tag,omitempty"`
	DryRun  bool     `json:"dryRun,omitempty"`
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

type CrawlRequest struct {
	URL                string                 `json:"url"`
	Preset             string                 `json:"preset,omitempty"`
	Prompt             string                 `json:"prompt,omitempty"`
	ScheduleAt         *time.Time             `json:"scheduleAt,omitempty"`
	IncludePaths       []string               `json:"includePaths,omitempty"`
	ExcludePaths       []string               `json:"excludePaths,omitempty"`
	MaxDiscoveryDepth  *int                   `json:"maxDiscoveryDepth,omitempty"`
	Limit              int                    `json:"limit,omitempty"`
	CrawlEntireDomain  bool                   `json:"crawlEntireDomain,omitempty"`
	AllowExternalLinks bool                   `json:"allowExternalLinks,omitempty"`
	AllowSubdomains    bool                   `json:"allowSubdomains,omitempty"`
	IgnoreRobotsTxt    bool                   `json:"ignoreRobotsTxt,omitempty"`
	Sitemap            string                 `json:"sitemap,omitempty"`
	ChangeTracking     *ChangeTrackingRequest `json:"changeTracking,omitempty"`
	ScrapeOptions      *ScrapeOptions         `json:"scrapeOptions,omitempty"`
	Webhook            *WebhookConfig         `json:"webhook,omitempty"`
}

type SearchRequest struct {
	Query         string         `json:"query"`
	Preset        string         `json:"preset,omitempty"`
	BlendMode     string         `json:"blendMode,omitempty"`
	Limit         int            `json:"limit,omitempty"`
	Sources       []SearchSource `json:"sources,omitempty"`
	Scrape        bool           `json:"scrape,omitempty"`
	Formats       []string       `json:"formats,omitempty"`
	ScrapeOptions *ScrapeOptions `json:"scrapeOptions,omitempty"`
	TimeoutSec    int            `json:"timeout,omitempty"`
	Webhook       *WebhookConfig `json:"webhook,omitempty"`
}

type ExtractRequest struct {
	URLs               []string        `json:"urls,omitempty"`
	Preset             string          `json:"preset,omitempty"`
	Prompt             string          `json:"prompt,omitempty"`
	SystemPrompt       string          `json:"systemPrompt,omitempty"`
	Schema             json.RawMessage `json:"schema,omitempty"`
	EnableWebSearch    bool            `json:"enableWebSearch,omitempty"`
	Limit              int             `json:"limit,omitempty"`
	TimeoutSec         int             `json:"timeout,omitempty"`
	IgnoreInvalidURLs  bool            `json:"ignoreInvalidURLs,omitempty"`
	Sitemap            string          `json:"sitemap,omitempty"`
	IncludePaths       []string        `json:"includePaths,omitempty"`
	ExcludePaths       []string        `json:"excludePaths,omitempty"`
	IncludeSubdomains  bool            `json:"includeSubdomains,omitempty"`
	AllowExternalLinks bool            `json:"allowExternalLinks,omitempty"`
	IgnoreRobotsTxt    bool            `json:"ignoreRobotsTxt,omitempty"`
	MaxDiscoveryDepth  *int            `json:"maxDiscoveryDepth,omitempty"`
	Webhook            *WebhookConfig  `json:"webhook,omitempty"`
	ScrapeOptions      *ScrapeOptions  `json:"scrapeOptions,omitempty"`
}

type ResearchRequest struct {
	Query         string         `json:"query,omitempty"`
	Prompt        string         `json:"prompt,omitempty"`
	SystemPrompt  string         `json:"systemPrompt,omitempty"`
	Preset        string         `json:"preset,omitempty"`
	BlendMode     string         `json:"blendMode,omitempty"`
	Limit         int            `json:"limit,omitempty"`
	MaxIterations int            `json:"maxIterations,omitempty"`
	Sources       []SearchSource `json:"sources,omitempty"`
	ScrapeOptions *ScrapeOptions `json:"scrapeOptions,omitempty"`
	Webhook       *WebhookConfig `json:"webhook,omitempty"`
	TimeoutSec    int            `json:"timeout,omitempty"`
}
