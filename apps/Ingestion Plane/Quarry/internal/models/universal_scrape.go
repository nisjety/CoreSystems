package models

// UniversalScrapeRequest for scraping any website without hardcoded rules
type UniversalScrapeRequest struct {
	URL            string                 `json:"url" validate:"required,url"`
	Purpose        string                 `json:"purpose"` // e.g., "extract products", "get article content"
	MaxAge         int64                  `json:"maxAge,omitempty"`
	Timeout        int                    `json:"timeout,omitempty"`
	Proxy          string                 `json:"proxy,omitempty"`
	ChangeTracking *ChangeTrackingRequest `json:"changeTracking,omitempty"`
	CacheAnalysis  bool                   `json:"cacheAnalysis,omitempty"`  // Reuse analysis within TTL
	ScrollToBottom bool                   `json:"scrollToBottom,omitempty"` // Auto-scroll for dynamic content
}

// UniversalScrapeResult returns both structure analysis and extracted data
type UniversalScrapeResult struct {
	URL       string                 `json:"url"`
	Data      interface{}            `json:"data"`              // Actual extracted data
	Analysis  *UniversalPageAnalysis `json:"analysis"`          // How the page was understood
	RawHTML   string                 `json:"rawHTML,omitempty"` // Optional raw content
	Warning   string                 `json:"warning,omitempty"`
	Cached    bool                   `json:"cached"`
	Timestamp string                 `json:"timestamp"`
}

// UniversalPageAnalysis describes what was found and how it was extracted
type UniversalPageAnalysis struct {
	PageType       string   `json:"pageType"`
	Confidence     float64  `json:"confidence"`
	MainContent    string   `json:"mainContent,omitempty"`
	ItemCount      int      `json:"itemCount,omitempty"` // For list pages
	Fields         []string `json:"fields"`              // Extracted field names
	ExtractionTime int64    `json:"extractionTime"`      // Milliseconds
	Method         string   `json:"method"`              // "css_selectors", "javascript", "llm_guided", etc.
}

// SmartScrapeRequest combines analysis caching with extraction
type SmartScrapeRequest struct {
	URL              string `json:"url" validate:"required,url"`
	Purpose          string `json:"purpose"`                    // e.g., "list all products", "get article metadata"
	OnlyAnalyze      bool   `json:"onlyAnalyze"`                // Return analysis only without extraction
	AnalysisCacheKey string `json:"analysisCacheKey,omitempty"` // For reusing cached analysis
}

// SmartScrapeResponse includes both analysis and data
type SmartScrapeResponse struct {
	URL       string          `json:"url"`
	Analysis  *AnalysisResult `json:"analysis"`
	Data      interface{}     `json:"data,omitempty"`
	Script    string          `json:"script,omitempty"`
	Warning   string          `json:"warning,omitempty"`
	Timestamp string          `json:"timestamp"`
}

// AnalysisResult is the LLM's understanding of page structure
type AnalysisResult struct {
	PageType       string           `json:"pageType"`
	MainContent    string           `json:"mainContent"`
	Items          []ItemSelector   `json:"items"`
	Fields         []FieldExtractor `json:"fields"`
	SkipSelectors  []string         `json:"skipSelectors"`
	WaitCondition  string           `json:"waitCondition"`
	ScrollRequired bool             `json:"scrollRequired"`
	JavaScriptCode string           `json:"javaScriptCode"`
	Confidence     float64          `json:"confidence"`
}

// ItemSelector for list extraction
type ItemSelector struct {
	Name     string `json:"name"`
	Selector string `json:"selector"`
}

// FieldExtractor for individual fields
type FieldExtractor struct {
	Name      string `json:"name"`
	Selector  string `json:"selector"`
	Attribute string `json:"attribute"`
	Type      string `json:"type"`
	Optional  bool   `json:"optional"`
}
