package crawl

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// RedirectInfo contains information about a single redirect
type RedirectInfo struct {
	URL        string            `json:"url"`
	StatusCode int               `json:"status_code"`
	Method     string            `json:"method"`
	Headers    map[string]string `json:"headers,omitempty"`
	Location   string            `json:"location,omitempty"`
	Timestamp  time.Time         `json:"timestamp"`
	Duration   time.Duration     `json:"duration"`
	IPAddress  string            `json:"ip_address,omitempty"`
	TLS        bool              `json:"tls"`
	Error      string            `json:"error,omitempty"`
}

// RedirectChain represents a complete redirect chain
type RedirectChain struct {
	OriginalURL   string          `json:"original_url"`
	FinalURL      string          `json:"final_url"`
	Redirects     []*RedirectInfo `json:"redirects"`
	TotalDuration time.Duration   `json:"total_duration"`
	RedirectCount int             `json:"redirect_count"`
	MaxRedirects  int             `json:"max_redirects"`
	UserAgent     string          `json:"user_agent"`
	Success       bool            `json:"success"`
	Error         string          `json:"error,omitempty"`
	Timestamp     time.Time       `json:"timestamp"`
}

// RedirectTracer handles redirect chain analysis
type RedirectTracer struct {
	MaxRedirects   int
	Timeout        time.Duration
	UserAgent      string
	FollowHTTPS    bool
	FollowHTTP     bool
	CheckCerts     bool
	CollectHeaders []string
}

// NewRedirectTracer creates a new redirect tracer with default settings
func NewRedirectTracer() *RedirectTracer {
	return &RedirectTracer{
		MaxRedirects:   10,
		Timeout:        30 * time.Second,
		UserAgent:      "SecureURLChecker/1.0",
		FollowHTTPS:    true,
		FollowHTTP:     true,
		CheckCerts:     true,
		CollectHeaders: []string{"Server", "X-Powered-By", "X-Frame-Options", "Content-Security-Policy"},
	}
}

// TraceRedirects follows all redirects and builds a complete chain
func (rt *RedirectTracer) TraceRedirects(ctx context.Context, originalURL string) (*RedirectChain, error) {
	startTime := time.Now()

	chain := &RedirectChain{
		OriginalURL:  originalURL,
		FinalURL:     originalURL,
		Redirects:    make([]*RedirectInfo, 0),
		MaxRedirects: rt.MaxRedirects,
		UserAgent:    rt.UserAgent,
		Timestamp:    startTime,
	}

	// Create custom client that doesn't follow redirects
	client := &http.Client{
		Timeout: rt.Timeout,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}

	currentURL := originalURL
	redirectCount := 0

	for redirectCount < rt.MaxRedirects {
		redirectStart := time.Now()

		// Create request
		req, err := http.NewRequestWithContext(ctx, "HEAD", currentURL, nil)
		if err != nil {
			chain.Error = fmt.Sprintf("Failed to create request: %v", err)
			return chain, err
		}

		// Set user agent
		req.Header.Set("User-Agent", rt.UserAgent)

		// Make request
		resp, err := client.Do(req)
		duration := time.Since(redirectStart)

		redirectInfo := &RedirectInfo{
			URL:       currentURL,
			Method:    "HEAD",
			Timestamp: redirectStart,
			Duration:  duration,
			TLS:       strings.HasPrefix(currentURL, "https://"),
		}

		if err != nil {
			redirectInfo.Error = err.Error()
			chain.Redirects = append(chain.Redirects, redirectInfo)
			chain.Error = fmt.Sprintf("Request failed: %v", err)
			break
		}

		redirectInfo.StatusCode = resp.StatusCode

		// Collect headers
		if len(rt.CollectHeaders) > 0 {
			redirectInfo.Headers = make(map[string]string)
			for _, headerName := range rt.CollectHeaders {
				if value := resp.Header.Get(headerName); value != "" {
					redirectInfo.Headers[headerName] = value
				}
			}
		}

		resp.Body.Close()

		// Check if it's a redirect
		if resp.StatusCode >= 300 && resp.StatusCode < 400 {
			location := resp.Header.Get("Location")
			if location == "" {
				redirectInfo.Error = "Redirect response missing Location header"
				chain.Redirects = append(chain.Redirects, redirectInfo)
				break
			}

			redirectInfo.Location = location
			chain.Redirects = append(chain.Redirects, redirectInfo)

			// Parse and resolve the location URL
			parsedLocation, err := url.Parse(location)
			if err != nil {
				chain.Error = fmt.Sprintf("Invalid redirect location: %v", err)
				break
			}

			// Resolve relative URLs
			if !parsedLocation.IsAbs() {
				baseURL, err := url.Parse(currentURL)
				if err != nil {
					chain.Error = fmt.Sprintf("Invalid base URL: %v", err)
					break
				}
				parsedLocation = baseURL.ResolveReference(parsedLocation)
			}

			nextURL := parsedLocation.String()
			currentURL = nextURL
			redirectCount++
		} else {
			// Final destination reached
			chain.Redirects = append(chain.Redirects, redirectInfo)
			chain.Success = true
			break
		}
	}

	if redirectCount >= rt.MaxRedirects {
		chain.Error = fmt.Sprintf("Too many redirects (max: %d)", rt.MaxRedirects)
	}

	chain.FinalURL = currentURL
	chain.RedirectCount = redirectCount
	chain.TotalDuration = time.Since(startTime)

	return chain, nil
}

// GetSuspiciousPatterns analyzes the redirect chain for suspicious patterns
func (chain *RedirectChain) GetSuspiciousPatterns() []string {
	var patterns []string

	if chain.RedirectCount == 0 {
		return patterns
	}

	// Check for HTTPS downgrade
	if strings.HasPrefix(chain.OriginalURL, "https://") && strings.HasPrefix(chain.FinalURL, "http://") {
		patterns = append(patterns, "https_downgrade")
	}

	// Check for excessive redirects
	if chain.RedirectCount > 5 {
		patterns = append(patterns, "excessive_redirects")
	}

	// Check for domain changes
	originalDomain := extractDomain(chain.OriginalURL)
	finalDomain := extractDomain(chain.FinalURL)
	if originalDomain != finalDomain {
		patterns = append(patterns, "domain_change")
	}

	return patterns
}

// extractDomain extracts the domain from a URL
func extractDomain(rawURL string) string {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return ""
	}
	return parsed.Host
}
