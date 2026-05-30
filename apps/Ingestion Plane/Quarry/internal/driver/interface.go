package driver

import "context"

// ViewportConfig overrides the browser viewport dimensions.
type ViewportConfig struct {
	Width             int     `json:"width"`
	Height            int     `json:"height"`
	DeviceScaleFactor float64 `json:"deviceScaleFactor,omitempty"`
}

// GeoLocation overrides the browser geolocation.
type GeoLocation struct {
	Latitude  float64 `json:"latitude"`
	Longitude float64 `json:"longitude"`
	// Accuracy in metres; defaults to 50 when zero.
	Accuracy float64 `json:"accuracy,omitempty"`
}

// FetchOptions carries per-request options for driver.Fetch.
type FetchOptions struct {
	Headers  map[string]string
	WaitFor  int
	ProxyURL string

	// Browser emulation
	Mobile   bool            // Enable mobile device emulation (390×844, DPR 3)
	Viewport *ViewportConfig // Override viewport dimensions (works with and without Mobile)
	Location *GeoLocation    // Spoof browser geolocation
	BlockAds bool            // Block common ad/tracker networks via request interception

	// AutoScroll scrolls to the bottom of the page to trigger lazy-loaded
	// content (infinite scroll, images, etc.). Only effective for JS drivers.
	AutoScroll bool

	// BlockMedia blocks images, fonts, and media resources during fetch.
	// Reduces bandwidth and fetch time by 300-800 ms on media-heavy pages.
	// Safe for HTML/markdown extraction; do NOT set when taking screenshots.
	BlockMedia bool
}

// FetchResult holds the HTTP response data captured by a driver.
type FetchResult struct {
	URL         string
	Status      int
	ContentType string
	HTML        string
	RawHTML     string
	Links       []string
	Rendered    bool
}

// PageDriver is the abstraction over HTTP and browser-based fetching.
type PageDriver interface {
	Name() string
	Fetch(ctx context.Context, targetURL string, opts *FetchOptions) (*FetchResult, error)
	HTML(ctx context.Context) (string, error)
	Click(ctx context.Context, selector string) error
	Type(ctx context.Context, selector, text string) error
	Press(ctx context.Context, key string) error
	Wait(ctx context.Context, milliseconds int) error
	Scroll(ctx context.Context, direction string) error
	Screenshot(ctx context.Context, fullPage bool) ([]byte, error)
	// EvalJS evaluates a JavaScript expression in the page context and returns its value.
	EvalJS(ctx context.Context, script string) (interface{}, error)
	// GeneratePDF renders the current page as a PDF and returns the raw bytes.
	GeneratePDF(ctx context.Context) ([]byte, error)
	Close() error
}
