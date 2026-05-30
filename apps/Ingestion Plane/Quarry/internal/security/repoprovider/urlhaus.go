package repoprovider

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// URLHausProvider implements URLHaus API for malware URL detection
type URLHausProvider struct {
	BaseURL    string
	HTTPClient *http.Client
}

// URLHausResponse represents the URLHaus API response
type URLHausResponse struct {
	QueryStatus string `json:"query_status"`
	ID          string `json:"id"`
	URLStatus   string `json:"url_status"`
	Host        string `json:"host"`
	DateAdded   string `json:"date_added"`
	Threat      string `json:"threat"`
	Blacklists  struct {
		SpamhausDBL string `json:"spamhaus_dbl"`
		SurblDBL    string `json:"surbl"`
	} `json:"blacklists"`
	Reporter     string   `json:"reporter"`
	Larted       string   `json:"larted"`
	Tags         []string `json:"tags"`
	URLStatusMap map[string]string
}

// NewURLHausProvider creates a new URLHaus provider
func NewURLHausProvider() *URLHausProvider {
	return &URLHausProvider{
		BaseURL: "https://urlhaus-api.abuse.ch/v1",
		HTTPClient: &http.Client{
			Timeout: 15 * time.Second,
		},
	}
}

// CheckURL checks a URL against URLHaus database
func (uhp *URLHausProvider) CheckURL(ctx context.Context, url string) (*ReputationResult, error) {
	startTime := time.Now()

	result := &ReputationResult{
		Provider:   "URLHaus",
		URL:        url,
		CheckedAt:  startTime,
		Level:      Clean,
		Confidence: 0.85,
	}

	// Create form data for POST request
	formData := fmt.Sprintf("url=%s", url)

	// Create HTTP request
	req, err := http.NewRequestWithContext(ctx, "POST", uhp.BaseURL+"/url/",
		strings.NewReader(formData))
	if err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}

	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("User-Agent", "SecureURLChecker/1.0")

	// Make request
	resp, err := uhp.HTTPClient.Do(req)
	if err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}
	defer resp.Body.Close()

	result.ResponseTime = time.Since(startTime)

	// Handle HTTP errors
	if resp.StatusCode != http.StatusOK {
		result.Error = fmt.Sprintf("HTTP %d", resp.StatusCode)
		result.Level = Unknown
		return result, fmt.Errorf("API returned HTTP %d", resp.StatusCode)
	}

	// Parse response
	var response URLHausResponse
	if err := json.NewDecoder(resp.Body).Decode(&response); err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}

	// Process results
	switch response.QueryStatus {
	case "ok":
		// URL found in database - this is bad
		result.Level = Blacklisted
		result.Score = 1.0
		result.Description = fmt.Sprintf("Malware URL: %s", response.Threat)

		if len(response.Tags) > 0 {
			result.Categories = response.Tags
		}

		// Add metadata
		result.Metadata = make(map[string]string)
		result.Metadata["threat_type"] = response.Threat
		result.Metadata["url_status"] = response.URLStatus
		result.Metadata["date_added"] = response.DateAdded
		result.Metadata["reporter"] = response.Reporter

		// Check blacklist status
		if response.Blacklists.SpamhausDBL != "" {
			result.Metadata["spamhaus_dbl"] = response.Blacklists.SpamhausDBL
		}
		if response.Blacklists.SurblDBL != "" {
			result.Metadata["surbl"] = response.Blacklists.SurblDBL
		}

	case "no_results":
		// URL not found in database - this is good
		result.Level = Clean
		result.Score = 0.0
		result.Description = "URL not found in malware database"

	case "invalid_url":
		result.Level = Unknown
		result.Score = 0.0
		result.Error = "Invalid URL format"
		result.Description = "URL format is invalid"

	default:
		result.Level = Unknown
		result.Score = 0.0
		result.Error = fmt.Sprintf("Unknown query status: %s", response.QueryStatus)
	}

	return result, nil
}

// CheckDomain checks a domain against URLHaus database
func (uhp *URLHausProvider) CheckDomain(ctx context.Context, domain string) (*ReputationResult, error) {
	startTime := time.Now()

	result := &ReputationResult{
		Provider:   "URLHaus",
		Domain:     domain,
		CheckedAt:  startTime,
		Level:      Clean,
		Confidence: 0.85,
	}

	// Create form data for POST request
	formData := fmt.Sprintf("host=%s", domain)

	// Create HTTP request
	req, err := http.NewRequestWithContext(ctx, "POST", uhp.BaseURL+"/host/",
		strings.NewReader(formData))
	if err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}

	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	req.Header.Set("User-Agent", "SecureURLChecker/1.0")

	// Make request
	resp, err := uhp.HTTPClient.Do(req)
	if err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}
	defer resp.Body.Close()

	result.ResponseTime = time.Since(startTime)

	// Handle HTTP errors
	if resp.StatusCode != http.StatusOK {
		result.Error = fmt.Sprintf("HTTP %d", resp.StatusCode)
		result.Level = Unknown
		return result, fmt.Errorf("API returned HTTP %d", resp.StatusCode)
	}

	// Parse response
	var response struct {
		QueryStatus string `json:"query_status"`
		Host        string `json:"host"`
		FirstSeen   string `json:"firstseen"`
		URLCount    int    `json:"url_count"`
		Blacklists  struct {
			SpamhausDBL string `json:"spamhaus_dbl"`
			SurblDBL    string `json:"surbl"`
		} `json:"blacklists"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&response); err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}

	// Process results
	switch response.QueryStatus {
	case "ok":
		// Host found in database - suspicious
		if response.URLCount > 0 {
			result.Level = Blacklisted
			result.Score = 0.9
			result.Description = fmt.Sprintf("Host serves %d malware URLs", response.URLCount)
		} else {
			result.Level = Suspicious
			result.Score = 0.3
			result.Description = "Host previously associated with malware"
		}

		// Add metadata
		result.Metadata = make(map[string]string)
		result.Metadata["url_count"] = fmt.Sprintf("%d", response.URLCount)
		result.Metadata["first_seen"] = response.FirstSeen

		if response.Blacklists.SpamhausDBL != "" {
			result.Metadata["spamhaus_dbl"] = response.Blacklists.SpamhausDBL
		}
		if response.Blacklists.SurblDBL != "" {
			result.Metadata["surbl"] = response.Blacklists.SurblDBL
		}

	case "no_results":
		// Host not found - clean
		result.Level = Clean
		result.Score = 0.0
		result.Description = "Host not found in malware database"

	default:
		result.Level = Unknown
		result.Score = 0.0
		result.Error = fmt.Sprintf("Unknown query status: %s", response.QueryStatus)
	}

	return result, nil
}

// Name returns the provider name
func (uhp *URLHausProvider) Name() string {
	return "URLHaus"
}

// IsAvailable checks if the provider is available
func (uhp *URLHausProvider) IsAvailable() bool {
	return true // URLHaus is freely available
}

// GetRateLimit returns the rate limit information
func (uhp *URLHausProvider) GetRateLimit() RateLimit {
	return RateLimit{
		RequestsPerMinute: 100, // Conservative estimate
		RequestsPerHour:   1000,
		RequestsPerDay:    10000,
		BurstAllowed:      5,
	}
}
