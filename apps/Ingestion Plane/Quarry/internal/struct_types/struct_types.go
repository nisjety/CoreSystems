package structtypes

type MultiLinkCrawlRequest struct {
	URLs     []string `json:"urls"`
	MaxDepth int      `json:"max_depth"`
}

type SingleLinkCrawlRequest struct {
	URL string `json:"url"`
}

// LinkInfo represents information about a link
type LinkInfo struct {
	Text        string `json:"text"`
	URL         string `json:"url"`
	Description string `json:"description"`
	IsInternal  bool   `json:"is_internal"`
	StatusCode  int    `json:"status_code"`
}

// URLCheckResult represents the result of checking a single URL
type URLCheckResult struct {
	URL          string     `json:"url"`
	StatusCode   int        `json:"status_code"`
	Success      bool       `json:"success"`
	ErrorMessage string     `json:"error_message,omitempty"`
	ResponseTime int64      `json:"response_time,omitempty"` // in milliseconds
	AIAnalysis   AIAnalysis `json:"ai_analysis,omitempty"`
}

// AIAnalysis represents AI-powered analysis of URL check results
type AIAnalysis struct {
	Description     string            `json:"description"`
	SiteType        string            `json:"site_type"`
	StatusMeaning   string            `json:"status_meaning"`
	Recommendations []string          `json:"recommendations,omitempty"`
	Confidence      float64           `json:"confidence"`
	Metadata        map[string]string `json:"metadata,omitempty"`
}

// MultiLinkCrawlResponse represents the response structure
type MultiLinkCrawlResponse struct {
	Success       bool             `json:"success"`
	TotalLinks    int              `json:"total_links"`
	InternalLinks []LinkInfo       `json:"internal_links"`
	ExternalLinks []LinkInfo       `json:"external_links"`
	InvalidLinks  []LinkInfo       `json:"invalid_links"`
	UrlResults    []CrawlURLResult `json:"url_results"`
	Summary       CrawlSummary     `json:"summary"`
	ErrorMessage  string           `json:"error_message,omitempty"`
}

// CrawlSummary provides a summary of the crawling operation
type CrawlSummary struct {
	TotalUrls      int         `json:"total_urls"`
	SuccessfulUrls int         `json:"successful_urls"`
	FailedUrls     int         `json:"failed_urls"`
	StatusCodes    map[int]int `json:"status_codes"`
}

// SingleLinkCrawlResponse represents the response structure
type SingleLinkCrawlResponse struct {
	Success       bool       `json:"success"`
	TotalLinks    int        `json:"total_links"`
	InternalLinks []LinkInfo `json:"internal_links"`
	ExternalLinks []LinkInfo `json:"external_links"`
	InvalidLinks  []LinkInfo `json:"invalid_links"`
	PageTitle     string     `json:"page_title,omitempty"`
	ErrorMessage  string     `json:"error_message,omitempty"`
}

// CrawlURLResult represents the result of crawling a single URL
type CrawlURLResult struct {
	Success       bool       `json:"success"`
	URL           string     `json:"url"`
	PageTitle     string     `json:"page_title"`
	TotalLinks    int        `json:"total_links"`
	InternalLinks []LinkInfo `json:"internal_links"`
	ExternalLinks []LinkInfo `json:"external_links"`
	InvalidLinks  []LinkInfo `json:"invalid_links"`
	ErrorMessage  string     `json:"error_message,omitempty"`
}

type SEOCrawlRequest struct {
	URL string `json:"url"`
}
