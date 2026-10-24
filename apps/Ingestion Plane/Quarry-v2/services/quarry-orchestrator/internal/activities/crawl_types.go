package activities

// The crawl scope wire types (WS14.S3). They mirror quarry-core's
// `crawl_scope` module: the orchestrator sends a CrawlContext with each page of
// a crawl, and the edge answers with a CrawlReport. Field names are the wire
// names; a Go nil slice would encode as `null`, which the edge refuses, so
// every slice is `omitempty`.

// CrawlScope is the scope of one crawl.
type CrawlScope struct {
	AllowSubdomains    bool     `json:"allow_subdomains"`
	AllowExternalLinks bool     `json:"allow_external_links"`
	CrawlEntireDomain  bool     `json:"crawl_entire_domain"`
	IncludePaths       []string `json:"include_paths,omitempty"`
	ExcludePaths       []string `json:"exclude_paths,omitempty"`
	DenyExtensions     []string `json:"deny_extensions,omitempty"`
	// Legacy is set by the handoff for a crawl accepted with the scope switch
	// off. The workflow then sends no context and runs as it did before scope.
	Legacy bool `json:"legacy,omitempty"`
}

// Crawl kinds and the scope and robots values a context may carry.
const (
	CrawlKindCrawl = "crawl"
	CrawlKindBatch = "batch"

	ScopeModeEnforce = "enforce"
	ScopeModeObserve = "observe"

	RobotsRespect = "respect"
	RobotsIgnore  = "ignore"

	// CrawlContractVersion is quarry-core CRAWL_CONTRACT_VERSION.
	CrawlContractVersion = 1
)

// CrawlContext is the `crawl` object of a run_page call.
type CrawlContext struct {
	V         int        `json:"v"`
	Kind      string     `json:"kind"`
	Depth     uint32     `json:"depth"`
	MaxDepth  uint32     `json:"max_depth"`
	SeedHosts []string   `json:"seed_hosts,omitempty"`
	SeedURL   string     `json:"seed_url,omitempty"`
	Scope     CrawlScope `json:"scope"`
	ScopeMode string     `json:"scope_mode"`
	Robots    string     `json:"robots"`
}

// DenialSample is one denied link in a report.
type DenialSample struct {
	URL     string  `json:"url"`
	Code    string  `json:"code"`
	Message string  `json:"message"`
	Remedy  *string `json:"remedy,omitempty"`
	Depth   uint32  `json:"depth"`
}

// RobotsDenial is one page the robots rules refused, with the token that refused it.
type RobotsDenial struct {
	URL       string `json:"url"`
	Code      string `json:"code"`
	UserAgent string `json:"user_agent"`
	Path      string `json:"path"`
}

// DenialReport is the denial part of a CrawlReport.
type DenialReport struct {
	Counts  map[string]uint32 `json:"counts"`
	Samples []DenialSample    `json:"samples"`
	Robots  []RobotsDenial    `json:"robots"`
}

// CrawlReport is what the edge answers for a page run with a `crawl` context.
// ScopeApplied is the proof that the edge understood the context: an orchestrator
// that gets a report without it ends the run (version skew, fail closed).
type CrawlReport struct {
	V                int          `json:"v"`
	ScopeApplied     bool         `json:"scope_applied"`
	Mode             string       `json:"mode"`
	LinksSeen        uint32       `json:"links_seen"`
	LinksAllowed     uint32       `json:"links_allowed"`
	LinksDenied      uint32       `json:"links_denied"`
	LinksUnevaluated uint32       `json:"links_unevaluated"`
	Denials          DenialReport `json:"denials"`
	HostsAdded       []string     `json:"hosts_added,omitempty"`
	CrawlDelayMS     *uint64      `json:"crawl_delay_ms,omitempty"`
}
