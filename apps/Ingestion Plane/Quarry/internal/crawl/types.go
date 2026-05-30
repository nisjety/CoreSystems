package crawl

import (
	"time"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/driver"
	"github.com/triodelab/quarry/internal/models"
)

type SitemapMode string

const (
	SitemapSkip    SitemapMode = "skip"
	SitemapInclude SitemapMode = "include"
	SitemapOnly    SitemapMode = "only"
)

type RunStatus string

const (
	StatusQueued    RunStatus = "queued"
	StatusRunning   RunStatus = "running"
	StatusCompleted RunStatus = "completed"
	StatusFailed    RunStatus = "failed"
	StatusCancelled RunStatus = "cancelled"
)

type Format struct {
	Type   string `json:"type"`
	Schema string `json:"schema,omitempty"`
	Prompt string `json:"prompt,omitempty"`
}

type PageOptions struct {
	Formats         []Format               `json:"formats,omitempty"`
	Headers         map[string]string      `json:"headers,omitempty"`
	WaitFor         int                    `json:"waitFor,omitempty"`
	OnlyMainContent bool                   `json:"onlyMainContent,omitempty"`
	IncludeTags     []string               `json:"includeTags,omitempty"`
	ExcludeTags     []string               `json:"excludeTags,omitempty"`
	Mobile          bool                   `json:"mobile,omitempty"`
	Viewport        *driver.ViewportConfig `json:"viewport,omitempty"`
	Location        *driver.GeoLocation    `json:"location,omitempty"`
	BlockAds        bool                   `json:"blockAds,omitempty"`
	ProxyURL        string                 `json:"proxyUrl,omitempty"`
	Actions         []actions.ActionStep   `json:"actions,omitempty"`
	MaxAgeMs        int64                  `json:"maxAge,omitempty"`
	ParserMode      string                 `json:"parserMode,omitempty"`
	RenderJS        *bool                  `json:"renderJs,omitempty"`
	AutoScroll      bool                   `json:"autoScroll,omitempty"`
}

type Spec struct {
	URL                    string                        `json:"url"`
	Preset                 string                        `json:"preset,omitempty"`
	IncludePaths           []string                      `json:"includePaths,omitempty"`
	ExcludePaths           []string                      `json:"excludePaths,omitempty"`
	MaxDiscoveryDepth      *int                          `json:"maxDiscoveryDepth,omitempty"`
	Limit                  int                           `json:"limit"`
	CrawlEntireDomain      bool                          `json:"crawlEntireDomain,omitempty"`
	AllowExternalLinks     bool                          `json:"allowExternalLinks,omitempty"`
	AllowSubdomains        bool                          `json:"allowSubdomains,omitempty"`
	IgnoreRobotsTxt        bool                          `json:"ignoreRobotsTxt,omitempty"`
	Sitemap                SitemapMode                   `json:"sitemap,omitempty"`
	DeduplicateSimilarURLs bool                          `json:"deduplicateSimilarURLs,omitempty"`
	IgnoreQueryParameters  bool                          `json:"ignoreQueryParameters,omitempty"`
	RegexOnFullURL         bool                          `json:"regexOnFullURL,omitempty"`
	RegexPaths             bool                          `json:"regexPaths,omitempty"`
	Delay                  time.Duration                 `json:"delay,omitempty"`
	MaxConcurrency         int                           `json:"maxConcurrency,omitempty"`
	Prompt                 string                        `json:"prompt,omitempty"`
	Schema                 string                        `json:"schema,omitempty"`
	Module                 string                        `json:"module,omitempty"`
	Enrich                 bool                          `json:"enrich,omitempty"`
	EnrichLimit            int                           `json:"enrichLimit,omitempty"`
	MaxAge                 int64                         `json:"maxAge,omitempty"`
	ScheduleAt             *time.Time                    `json:"scheduleAt,omitempty"`
	ChangeTracking         *models.ChangeTrackingRequest `json:"changeTracking,omitempty"`
	PageOptions            PageOptions                   `json:"scrapeOptions,omitempty"`
	ZDRMode                bool                          `json:"-"`
	DiscoveryOnly          bool                          `json:"-"`
	OrgID                  string                        `json:"-"` // forwarded to AI agent for billing context
}

type Run struct {
	ID        string                 `json:"id"`
	URL       string                 `json:"url"`
	Status    RunStatus              `json:"status"`
	CreatedAt time.Time              `json:"createdAt"`
	UpdatedAt time.Time              `json:"updatedAt"`
	ExpiresAt time.Time              `json:"expiresAt"`
	Queued    int                    `json:"queued"`
	Active    int                    `json:"active"`
	Completed int                    `json:"completed"`
	Failed    int                    `json:"failed"`
	Blocked   int                    `json:"blocked"`
	Total     int                    `json:"total"`
	Warning   string                 `json:"warning,omitempty"`
	Spec      Spec                   `json:"spec"`
	Meta      map[string]interface{} `json:"meta,omitempty"`
}

type Document struct {
	URL      string                 `json:"url"`
	Metadata map[string]interface{} `json:"metadata,omitempty"`
	Outputs  map[string]interface{} `json:"outputs,omitempty"`
}

type PageError struct {
	URL       string `json:"url"`
	Code      string `json:"code,omitempty"`
	Error     string `json:"error"`
	Timestamp string `json:"timestamp,omitempty"`
}

type Item struct {
	URL       string
	Depth     int
	SourceURL string
}

type FetchedPage struct {
	URL         string
	StatusCode  int
	ContentType string
	Links       []string
	Outputs     map[string]interface{}
	Metadata    map[string]interface{}
}
