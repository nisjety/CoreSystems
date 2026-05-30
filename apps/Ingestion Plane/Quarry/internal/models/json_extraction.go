package models

// ExtractRequest represents a request to extract structured data from a page
type ExtractRequest struct {
	URL            string                 `json:"url" validate:"required,url"`
	Schema         map[string]interface{} `json:"schema,omitempty"`         // JSON Schema for structured extraction
	Prompt         string                 `json:"prompt,omitempty"`         // Alternative: free-form prompt
	Proxy          string                 `json:"proxy,omitempty"`          // Proxy strategy: "basic" | "stealth" | "auto"
	MaxAge         int64                  `json:"maxAge,omitempty"`         // Cache age in milliseconds
	Timeout        int                    `json:"timeout,omitempty"`        // Timeout in milliseconds
	MainOnly       bool                   `json:"mainOnly,omitempty"`       // Extract only from main content
	ChangeTracking *ChangeTrackingRequest `json:"changeTracking,omitempty"` // Change tracking options
}

// JSONFormat controls schema/prompt-based extraction behavior.
type JSONFormat struct {
	Schema map[string]interface{} `json:"schema,omitempty"`
	Prompt string                 `json:"prompt,omitempty"`
}

// ExtractResult represents extracted structured data
type ExtractResult struct {
	URL            string                 `json:"url"`
	Data           map[string]interface{} `json:"data"`
	Metadata       *PageMetadata          `json:"metadata,omitempty"`
	ChangeTracking *ChangeTrackingResult  `json:"changeTracking,omitempty"` // Change tracking results
	Warning        string                 `json:"warning,omitempty"`
	Cached         bool                   `json:"cached"`
	Timestamp      string                 `json:"timestamp"`
}

// PageMetadata contains metadata about the scraped page
type PageMetadata struct {
	Title       string `json:"title,omitempty"`
	Description string `json:"description,omitempty"`
	SourceURL   string `json:"source_url"`
	StatusCode  int    `json:"status_code,omitempty"`
}
